//! Texture loads, samples and queries.

use texelscope_capture::{DecodedTexture, Sampler};
use texelscope_ir::naga::{
    self, DerivativeAxis, DerivativeControl, Expression, Handle, ImageQuery, SampleLevel,
};

use super::{Computed, Extra, Lane};
use crate::image::{Fetch, TexelNudge};
use crate::resources::Global;
use crate::trace::Origin;
use crate::val::{Scalar, Val, Vector};
use crate::{InterpError, Interpreter, Perturbation};

impl<'a> Lane<'a> {
    pub(super) fn texture(
        &self,
        image: Handle<Expression>,
    ) -> Result<(&'a DecodedTexture, u32, u32, Handle<naga::GlobalVariable>), InterpError> {
        let Val::Image(g) = *self.value(image)? else {
            return Err(InterpError::Internal("expected a texture".into()));
        };
        let interp: &'a Interpreter = self.interp;
        match &interp.globals[g.index()] {
            Global::Texture {
                texture,
                group,
                binding,
            } => Ok((texture.as_ref(), *group, *binding, g)),
            other => Err(self.interp.global_error(g, other)),
        }
    }

    pub(super) fn sampler(&self, sampler: Handle<Expression>) -> Result<&'a Sampler, InterpError> {
        let Val::Sampler(g) = *self.value(sampler)? else {
            return Err(InterpError::Internal("expected a sampler".into()));
        };
        let interp: &'a Interpreter = self.interp;
        match &interp.globals[g.index()] {
            Global::Sampler(sampler) => Ok(sampler),
            other => Err(self.interp.global_error(g, other)),
        }
    }

    pub(super) fn fetch<'t>(
        &self,
        texture: &'t DecodedTexture,
        group: u32,
        binding: u32,
    ) -> Fetch<'t> {
        let nudge = match self.nudge {
            Some(Perturbation::Texel {
                group: g,
                binding: b,
                level,
                texel,
                component,
                delta,
            }) if (*g, *b) == (group, binding) => Some(TexelNudge {
                level: *level,
                texel: *texel,
                component: *component as usize,
                delta: *delta,
            }),
            _ => None,
        };
        Fetch::new(texture, nudge, self.record.is_on())
    }

    pub(super) fn texels_extra(
        &self,
        fetch: &Fetch,
        group: u32,
        binding: u32,
        g: Handle<naga::GlobalVariable>,
    ) -> Extra {
        let name = self.module().global_variables[g]
            .name
            .clone()
            .unwrap_or_default();
        Extra {
            origin: Some(Origin::Texels {
                group,
                binding,
                name,
                level: fetch.level,
                texels: fetch.texels.clone(),
            }),
            note: fetch
                .clamped
                .then(|| "a coordinate was outside the texture and clamped".to_string()),
            inputs: None,
        }
    }

    pub(super) fn image_load(
        &mut self,
        image: Handle<Expression>,
        coordinate: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        level: Option<Handle<Expression>>,
    ) -> Result<Computed, InterpError> {
        self.ensure(image)?;
        let coord = self.owned(coordinate)?;
        let layer = array_index.map(|h| self.owned(h)).transpose()?;
        let level = level.map(|h| self.owned(h)).transpose()?;
        let (texture, group, binding, g) = self.texture(image)?;
        let lanes = coord.lanes().unwrap_or_default();
        let at = |i: usize| lanes.get(i).map_or(0, |s| s.as_i64());
        let mut fetch = self.fetch(texture, group, binding);
        let texel = fetch.load(
            [at(0), at(1), at(2)],
            layer.map(|l| l.scalar().map(Scalar::as_i64)).transpose()?,
            level
                .map(|l| l.scalar().map(Scalar::as_i64))
                .transpose()?
                .unwrap_or(0),
        );
        let extra = self.texels_extra(&fetch, group, binding, g);
        Ok(Computed::Value(Val::vec_f32(&texel), extra))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn image_sample(
        &mut self,
        image: Handle<Expression>,
        sampler: Handle<Expression>,
        gather: Option<naga::SwizzleComponent>,
        coordinate: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        offset: Option<Handle<Expression>>,
        level: SampleLevel,
        clamp_to_edge: bool,
    ) -> Result<Computed, InterpError> {
        self.ensure(image)?;
        self.ensure(sampler)?;
        let coord_val = self.owned(coordinate)?;
        let coord = floats3(&coord_val);
        let layer = array_index
            .map(|h| self.owned(h).and_then(|v| v.scalar()).map(Scalar::as_i64))
            .transpose()?;
        let offset = match offset {
            Some(h) => {
                let v = self.owned(h)?;
                let lanes = v.lanes().unwrap_or_default();
                std::array::from_fn(|i| lanes.get(i).map_or(0, |s| s.as_i64() as i32))
            }
            None => [0; 3],
        };
        let (texture, group, binding, g) = self.texture(image)?;
        let sampler = *self.sampler(sampler)?;
        let mut fetch = self.fetch(texture, group, binding);

        if let Some(component) = gather {
            let texel = fetch.gather(&sampler, coord, layer, offset, component as usize);
            let extra = self.texels_extra(&fetch, group, binding, g);
            return Ok(Computed::Value(Val::vec_f32(&texel), extra));
        }

        let mut coord = coord;
        let lod = match level {
            SampleLevel::Zero => 0.0,
            SampleLevel::Exact(h) => self.owned(h)?.scalar()?.as_f32(),
            SampleLevel::Gradient { x, y } => {
                let (dx, dy) = (floats3(&self.owned(x)?), floats3(&self.owned(y)?));
                fetch.implicit_lod(&dx, &dy)
            }
            SampleLevel::Auto | SampleLevel::Bias(_) => {
                let bias = match level {
                    SampleLevel::Bias(h) => self.owned(h)?.scalar()?.as_f32(),
                    _ => 0.0,
                };
                // Only a choice between filters or mip levels needs the quad.
                let needs_quad = fetch.levels() > 1 || sampler.min_filter != sampler.mag_filter;
                if !needs_quad {
                    0.0
                } else {
                    match self.resume.take() {
                        None => return Ok(Computed::Wait(coord_val)),
                        Some(quad) => {
                            let dx = floats3(&self.derivative(
                                &quad,
                                DerivativeAxis::X,
                                DerivativeControl::Fine,
                            )?);
                            let dy = floats3(&self.derivative(
                                &quad,
                                DerivativeAxis::Y,
                                DerivativeControl::Fine,
                            )?);
                            fetch.implicit_lod(&dx, &dy) + bias
                        }
                    }
                }
            }
        };
        if clamp_to_edge {
            let [w, h, _] = fetch.size(0);
            coord[0] = coord[0].clamp(0.5 / w as f32, 1.0 - 0.5 / w as f32);
            coord[1] = coord[1].clamp(0.5 / h as f32, 1.0 - 0.5 / h as f32);
        }
        let texel = fetch.sample(&sampler, coord, layer, offset, lod);
        let extra = self.texels_extra(&fetch, group, binding, g);
        Ok(Computed::Value(Val::vec_f32(&texel), extra))
    }

    pub(super) fn image_query(
        &mut self,
        image: Handle<Expression>,
        query: ImageQuery,
    ) -> Result<Computed, InterpError> {
        self.ensure(image)?;
        let (texture, group, binding, _) = self.texture(image)?;
        let fetch = self.fetch(texture, group, binding);
        let image_type = self.image_type(image);
        let value = match query {
            ImageQuery::Size { level } => {
                let level = level
                    .map(|h| self.owned(h).and_then(|v| v.scalar()).map(Scalar::as_i64))
                    .transpose()?
                    .unwrap_or(0)
                    .clamp(0, fetch.levels() as i64 - 1) as usize;
                let [w, h, d] = fetch.size(level);
                let dims: &[u32] = match image_type {
                    Some((naga::ImageDimension::D1, _)) => &[w],
                    Some((naga::ImageDimension::D3, _)) => &[w, h, d],
                    _ => &[w, h],
                };
                if dims.len() == 1 {
                    Val::Scalar(Scalar::U32(dims[0]))
                } else {
                    let lanes: Vec<Scalar> = dims.iter().map(|&n| Scalar::U32(n)).collect();
                    Val::Vector(Vector::new(&lanes))
                }
            }
            ImageQuery::NumLevels => Val::Scalar(Scalar::U32(fetch.levels() as u32)),
            ImageQuery::NumLayers => Val::Scalar(Scalar::U32(fetch.size(0)[2])),
            ImageQuery::NumSamples => Val::Scalar(Scalar::U32(1)),
        };
        Ok(Computed::Value(value, Extra::default()))
    }

    pub(super) fn image_type(
        &self,
        image: Handle<Expression>,
    ) -> Option<(naga::ImageDimension, bool)> {
        let Ok(Val::Image(g)) = self.value(image) else {
            return None;
        };
        let module = self.module();
        match module.types[module.global_variables[*g].ty].inner {
            naga::TypeInner::Image { dim, arrayed, .. } => Some((dim, arrayed)),
            _ => None,
        }
    }
}

fn floats3(value: &Val) -> [f32; 3] {
    let lanes = value.lanes().unwrap_or_default();
    std::array::from_fn(|i| lanes.get(i).map_or(0.0, |s| s.as_f32()))
}
