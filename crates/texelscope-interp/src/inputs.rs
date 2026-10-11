//! The arguments a fragment entry point receives and the outputs its return value holds.

use texelscope_capture::FragmentInputs;
use texelscope_ir::naga::{self, Binding, BuiltIn, Handle, TypeInner};

use crate::trace::Origin;
use crate::val::{CompositeKind, Scalar, Val, Vector};
use crate::{InterpError, Interpreter, Perturbation};

/// Builds every entry point argument for a pixel, with where each came from.
pub(crate) fn entry_arguments(
    interp: &Interpreter,
    pixel: [u32; 2],
    nudge: Option<&Perturbation>,
    notes: &mut Vec<String>,
) -> Result<(Vec<Val>, Vec<Origin>), InterpError> {
    let module = interp.module.naga();
    let mut values = Vec::new();
    let mut origins = Vec::new();
    for arg in &interp.entry_ir().arguments {
        let name = arg.name.clone().unwrap_or_else(|| "input".to_string());
        match &arg.binding {
            Some(binding) => {
                let (value, label, location) =
                    bound_value(interp, binding, arg.ty, pixel, nudge, notes)?;
                values.push(value);
                origins.push(Origin::Input {
                    name: if label == name {
                        label
                    } else {
                        format!("{name} ({label})")
                    },
                    location,
                });
            }
            None => {
                let TypeInner::Struct { members, .. } = &module.types[arg.ty].inner else {
                    return Err(InterpError::Internal(format!(
                        "entry argument {name} has no binding"
                    )));
                };
                let items = members
                    .iter()
                    .map(|m| match &m.binding {
                        Some(binding) => {
                            bound_value(interp, binding, m.ty, pixel, nudge, notes).map(|(v, ..)| v)
                        }
                        None => Val::zero(module, m.ty),
                    })
                    .collect::<Result<_, _>>()?;
                values.push(Val::composite(CompositeKind::Struct(arg.ty), items));
                origins.push(Origin::Input {
                    name,
                    location: None,
                });
            }
        }
    }
    Ok((values, origins))
}

/// The value of one bound input, a name for it and its location.
fn bound_value(
    interp: &Interpreter,
    binding: &Binding,
    ty: Handle<naga::Type>,
    pixel: [u32; 2],
    nudge: Option<&Perturbation>,
    notes: &mut Vec<String>,
) -> Result<(Val, String, Option<u32>), InterpError> {
    let module = interp.module.naga();
    let target = interp.capture().target;
    let (label, location, mut lanes): (String, Option<u32>, Vec<f32>) = match *binding {
        Binding::BuiltIn(BuiltIn::Position { .. }) => {
            let lanes = match interp.data.window() {
                Some(window) => window_at(window, pixel, &window.frag_coord, 4)?,
                None => vec![pixel[0] as f32 + 0.5, pixel[1] as f32 + 0.5, 0.0, 1.0],
            };
            ("position".into(), None, lanes)
        }
        Binding::BuiltIn(BuiltIn::FrontFacing) => {
            return Ok((Val::Scalar(Scalar::Bool(true)), "front_facing".into(), None));
        }
        Binding::BuiltIn(BuiltIn::SampleIndex) => {
            return Ok((Val::Scalar(Scalar::U32(0)), "sample_index".into(), None));
        }
        Binding::BuiltIn(BuiltIn::SampleMask) => {
            return Ok((Val::Scalar(Scalar::U32(1)), "sample_mask".into(), None));
        }
        Binding::BuiltIn(other) => {
            notes.push(format!(
                "the {other:?} input is not modeled, so it reads zero"
            ));
            return Ok((Val::zero(module, ty)?, format!("{other:?}"), None));
        }
        Binding::Location {
            location,
            interpolation,
            ..
        } => {
            let label = format!("location({location})");
            if let Some(window) = interp.data.window() {
                let name = interp
                    .module
                    .glsl_layout()
                    .and_then(|l| l.inputs.iter().find(|i| i.location == location))
                    .map(|i| i.name.clone());
                let Some((name, (components, values))) =
                    name.and_then(|n| window.varyings.get(&n).map(|v| (n, v)))
                else {
                    notes.push(format!(
                        "the capture gives no values for location({location}), so it reads zero"
                    ));
                    return Ok((Val::zero(module, ty)?, label, Some(location)));
                };
                let lanes = window_at(window, pixel, values, *components)?;
                if lanes.iter().any(|v| v.is_nan()) {
                    notes.push("the draw didn't cover this pixel, so its inputs are NaN".into());
                }
                return Ok((shape(module, ty, &lanes)?, name, Some(location)));
            }
            let corners = match &interp.capture().inputs {
                FragmentInputs::Fullscreen { locations } => locations
                    .iter()
                    .find(|l| l.location == location)
                    .map(|l| &l.corners),
                FragmentInputs::Builtins | FragmentInputs::Window { .. } => None,
            };
            let Some(corners) = corners else {
                notes.push(format!(
                    "the capture gives no values for @location({location}), so it reads zero"
                ));
                return Ok((Val::zero(module, ty)?, label, Some(location)));
            };
            let s = (pixel[0] as f32 + 0.5) / target.width as f32;
            let t = (pixel[1] as f32 + 0.5) / target.height as f32;
            let [tl, tr, bl, br] = corners;
            let lanes = if interpolation == Some(naga::Interpolation::Flat) {
                tl.clone()
            } else {
                (0..tl.len())
                    .map(|i| {
                        let top = tl[i] + (tr[i] - tl[i]) * s;
                        let bottom = bl[i] + (br[i] - bl[i]) * s;
                        top + (bottom - top) * t
                    })
                    .collect()
            };
            (label, Some(location), lanes)
        }
    };
    if let Some(Perturbation::Input {
        name,
        component,
        delta,
    }) = nudge
        && *name == label
        && let Some(lane) = lanes.get_mut(*component as usize)
    {
        *lane += delta;
    }
    Ok((shape(module, ty, &lanes)?, label, location))
}

/// Fits interpolated floats to the declared input type.
fn shape(module: &naga::Module, ty: Handle<naga::Type>, lanes: &[f32]) -> Result<Val, InterpError> {
    let scalar = |kind: naga::ScalarKind, v: f32| -> Scalar {
        match kind {
            naga::ScalarKind::Sint => Scalar::I32(v.round() as i32),
            naga::ScalarKind::Uint => Scalar::U32(v.round().max(0.0) as u32),
            naga::ScalarKind::Bool => Scalar::Bool(v != 0.0),
            _ => Scalar::F32(v),
        }
    };
    let lane = |i: usize| lanes.get(i).copied().unwrap_or(0.0);
    Ok(match module.types[ty].inner {
        TypeInner::Scalar(s) => Val::Scalar(scalar(s.kind, lane(0))),
        TypeInner::Vector { size, scalar: s } => {
            let items: Vec<Scalar> = (0..size as usize)
                .map(|i| scalar(s.kind, lane(i)))
                .collect();
            Val::Vector(Vector::new(&items))
        }
        ref other => {
            return Err(InterpError::Unsupported(format!(
                "fragment inputs of type {other:?}"
            )));
        }
    })
}

/// An output's location or builtin name, and its value.
pub(crate) type RawOutput = (Option<u32>, Option<String>, Val);

/// Splits the entry point's return value into its outputs.
pub(crate) fn entry_outputs(
    interp: &Interpreter,
    value: Val,
) -> Result<Vec<RawOutput>, InterpError> {
    let module = interp.module.naga();
    let Some(result) = &interp.entry_ir().result else {
        return Ok(Vec::new());
    };
    let output = |binding: &Binding, value: Val| match *binding {
        Binding::Location { location, .. } => (Some(location), None, value),
        Binding::BuiltIn(b) => (None, Some(builtin_name(b)), value),
    };
    if let Some(binding) = &result.binding {
        return Ok(vec![output(binding, value)]);
    }
    let (TypeInner::Struct { members, .. }, Val::Composite(c)) =
        (&module.types[result.ty].inner, &value)
    else {
        return Err(InterpError::Internal(
            "an entry point result without bindings".into(),
        ));
    };
    Ok(members
        .iter()
        .zip(c.items.iter())
        .filter_map(|(m, v)| m.binding.as_ref().map(|b| output(b, v.clone())))
        .collect())
}

fn builtin_name(builtin: BuiltIn) -> String {
    match builtin {
        BuiltIn::FragDepth => "frag_depth".into(),
        BuiltIn::SampleMask => "sample_mask".into(),
        other => format!("{other:?}"),
    }
}

/// The floats a window input holds at a pixel. A quad neighbor just outside the window reads
/// the nearest pixel inside it.
fn window_at(
    window: &crate::resources::Window,
    pixel: [u32; 2],
    values: &[f32],
    components: u32,
) -> Result<Vec<f32>, InterpError> {
    let r = window.rect;
    let (x, y) = (i64::from(pixel[0]), i64::from(pixel[1]));
    let (x0, y0) = (i64::from(r.x), i64::from(r.y));
    let (x1, y1) = (x0 + i64::from(r.width) - 1, y0 + i64::from(r.height) - 1);
    if x < x0 - 1 || x > x1 + 1 || y < y0 - 1 || y > y1 + 1 {
        return Err(InterpError::BadData(format!(
            "pixel ({}, {}) is outside the {} by {} window at ({}, {}) the capture recorded",
            pixel[0], pixel[1], r.width, r.height, r.x, r.y
        )));
    }
    let n = components as usize;
    let at = ((y.clamp(y0, y1) - y0) * i64::from(r.width) + (x.clamp(x0, x1) - x0)) as usize * n;
    values
        .get(at..at + n)
        .map(<[f32]>::to_vec)
        .ok_or_else(|| InterpError::BadData("window inputs are shorter than their window".into()))
}
