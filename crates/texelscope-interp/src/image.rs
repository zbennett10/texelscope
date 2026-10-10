//! Texture reads with WebGPU's addressing, filtering and mip selection.

use texelscope_capture::{AddressMode, DecodedTexture, FilterMode, Sampler, TextureDimension};

/// A change to one texel component, used to measure how much a texel matters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TexelNudge {
    pub(crate) level: u32,
    pub(crate) texel: [i32; 3],
    pub(crate) component: usize,
    pub(crate) delta: f32,
}

/// Reads from one texture and remembers which texels it touched.
pub(crate) struct Fetch<'a> {
    pub(crate) texture: &'a DecodedTexture,
    pub(crate) nudge: Option<TexelNudge>,
    pub(crate) record: bool,
    pub(crate) texels: Vec<[i32; 3]>,
    pub(crate) level: u32,
    pub(crate) clamped: bool,
}

impl<'a> Fetch<'a> {
    pub(crate) fn new(
        texture: &'a DecodedTexture,
        nudge: Option<TexelNudge>,
        record: bool,
    ) -> Self {
        Self {
            texture,
            nudge,
            record,
            texels: Vec::new(),
            level: 0,
            clamped: false,
        }
    }

    pub(crate) fn levels(&self) -> usize {
        self.texture.levels.len()
    }

    pub(crate) fn size(&self, level: usize) -> [u32; 3] {
        self.texture.levels[level.min(self.levels() - 1)].size
    }

    fn texel(&mut self, level: usize, at: [u32; 3]) -> [f32; 4] {
        let mut value = self.texture.texel(level, at);
        let at = at.map(|c| c as i32);
        if let Some(n) = self.nudge
            && n.level as usize == level
            && n.texel == at
        {
            value[n.component] += n.delta;
        }
        if self.record && !self.texels.contains(&at) {
            self.texels.push(at);
        }
        value
    }

    /// `textureLoad`. Out-of-range coordinates clamp, as WebGPU's robust access does.
    pub(crate) fn load(&mut self, coord: [i64; 3], layer: Option<i64>, level: i64) -> [f32; 4] {
        let last_level = self.levels() as i64 - 1;
        let level = self.clamp(level, last_level) as usize;
        self.level = level as u32;
        let [w, h, d] = self.size(level);
        let x = self.clamp(coord[0], i64::from(w) - 1) as u32;
        let y = match self.texture.dimension {
            TextureDimension::D1 => 0,
            _ => self.clamp(coord[1], i64::from(h) - 1) as u32,
        };
        let z = match (self.texture.dimension, layer) {
            (TextureDimension::D3, _) => self.clamp(coord[2], i64::from(d) - 1) as u32,
            (_, Some(layer)) => self.clamp(layer, i64::from(d) - 1) as u32,
            _ => 0,
        };
        self.texel(level, [x, y, z])
    }

    /// `textureSample` and its variants at a level of detail already worked out.
    pub(crate) fn sample(
        &mut self,
        sampler: &Sampler,
        coord: [f32; 3],
        layer: Option<i64>,
        offset: [i32; 3],
        lod: f32,
    ) -> [f32; 4] {
        // Magnification uses the base level, minification walks the mip chain.
        let (filter, lod) = if lod.is_nan() || lod <= 0.0 {
            (sampler.mag_filter, 0.0)
        } else {
            (sampler.min_filter, lod.min(self.levels() as f32 - 1.0))
        };
        let layer = layer.map(|l| self.clamp(l, i64::from(self.size(0)[2]) - 1) as u32);
        match sampler.mipmap_filter {
            FilterMode::Nearest => {
                let level = (lod + 0.5).floor() as usize;
                self.sample_level(sampler, filter, level, coord, layer, offset)
            }
            FilterMode::Linear => {
                let base = lod.floor();
                let t = lod - base;
                let a = self.sample_level(sampler, filter, base as usize, coord, layer, offset);
                if t == 0.0 {
                    return a;
                }
                let next = (base as usize + 1).min(self.levels() - 1);
                let b = self.sample_level(sampler, filter, next, coord, layer, offset);
                std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
            }
        }
    }

    /// `textureGather`, returning one component of the four texels a bilinear tap reads.
    pub(crate) fn gather(
        &mut self,
        sampler: &Sampler,
        coord: [f32; 3],
        layer: Option<i64>,
        offset: [i32; 3],
        component: usize,
    ) -> [f32; 4] {
        let [w, h, _] = self.size(0);
        let z = layer.map_or(0, |l| self.clamp(l, i64::from(self.size(0)[2]) - 1) as u32);
        let x = coord[0] * w as f32 - 0.5;
        let y = coord[1] * h as f32 - 0.5;
        let (x0, y0) = (x.floor() as i32 + offset[0], y.floor() as i32 + offset[1]);
        let at = |fetch: &mut Self, i: i32, j: i32| {
            let tx = wrap(i, w, sampler.address_mode_u);
            let ty = wrap(j, h, sampler.address_mode_v);
            fetch.texel(0, [tx, ty, z])[component]
        };
        // WGSL orders the taps (umin, vmax), (umax, vmax), (umax, vmin), (umin, vmin).
        [
            at(self, x0, y0 + 1),
            at(self, x0 + 1, y0 + 1),
            at(self, x0 + 1, y0),
            at(self, x0, y0),
        ]
    }

    /// The level of detail implied by screen-space derivatives of the coordinate.
    pub(crate) fn implicit_lod(&self, ddx: &[f32], ddy: &[f32]) -> f32 {
        let size = self.size(0);
        let scaled = |d: &[f32]| -> f32 {
            d.iter()
                .zip(size)
                .map(|(v, n)| (v * n as f32).powi(2))
                .sum::<f32>()
                .sqrt()
        };
        scaled(ddx).max(scaled(ddy)).log2()
    }

    fn sample_level(
        &mut self,
        sampler: &Sampler,
        filter: FilterMode,
        level: usize,
        coord: [f32; 3],
        layer: Option<u32>,
        offset: [i32; 3],
    ) -> [f32; 4] {
        let level = level.min(self.levels() - 1);
        self.level = level as u32;
        let size = self.size(level);
        let axes = match self.texture.dimension {
            TextureDimension::D1 => 1,
            TextureDimension::D2 => 2,
            TextureDimension::D3 => 3,
        };
        let modes = [
            sampler.address_mode_u,
            sampler.address_mode_v,
            sampler.address_mode_w,
        ];
        // Each axis contributes one tap for nearest filtering and two for linear.
        let mut taps: [[(u32, f32); 2]; 3] = [[(0, 1.0), (0, 0.0)]; 3];
        for axis in 0..axes {
            let n = size[axis];
            let t = coord[axis] * n as f32;
            taps[axis] = match filter {
                FilterMode::Nearest => {
                    let i = t.floor() as i32 + offset[axis];
                    [(wrap(i, n, modes[axis]), 1.0), (0, 0.0)]
                }
                FilterMode::Linear => {
                    let t = t - 0.5;
                    let i = t.floor() as i32 + offset[axis];
                    let f = t - t.floor();
                    [
                        (wrap(i, n, modes[axis]), 1.0 - f),
                        (wrap(i + 1, n, modes[axis]), f),
                    ]
                }
            };
        }
        if axes < 3 {
            taps[2] = [(layer.unwrap_or(0), 1.0), (0, 0.0)];
        }
        let mut out = [0.0f32; 4];
        for &(z, wz) in &taps[2] {
            for &(y, wy) in &taps[1] {
                for &(x, wx) in &taps[0] {
                    let weight = wx * wy * wz;
                    if weight == 0.0 {
                        continue;
                    }
                    let texel = self.texel(level, [x, y, z]);
                    for (o, t) in out.iter_mut().zip(texel) {
                        *o += t * weight;
                    }
                }
            }
        }
        out
    }

    fn clamp(&mut self, value: i64, last: i64) -> i64 {
        let clamped = value.clamp(0, last.max(0));
        if clamped != value {
            self.clamped = true;
        }
        clamped
    }
}

fn wrap(i: i32, n: u32, mode: AddressMode) -> u32 {
    let n = n as i32;
    let i = match mode {
        AddressMode::ClampToEdge => i.clamp(0, n - 1),
        AddressMode::Repeat => i.rem_euclid(n),
        AddressMode::MirrorRepeat => {
            let t = i.rem_euclid(2 * n);
            if t < n { t } else { 2 * n - 1 - t }
        }
    };
    i as u32
}
