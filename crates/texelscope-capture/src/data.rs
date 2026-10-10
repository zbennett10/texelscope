//! The bytes behind buffer data and the texels behind texture data.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::{BufferData, CaptureError, Texture, TextureData, TextureDimension, TextureFormat};

/// One mip level of a decoded texture.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLevel {
    /// Width, height and depth or layer count.
    pub size: [u32; 3],
    /// Every texel as the shader reads it, by layer or slice, then rows from the top.
    pub texels: Vec<[f32; 4]>,
}

/// A texture with every mip level decoded to the values a shader reads.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedTexture {
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub levels: Vec<DecodedLevel>,
}

impl DecodedTexture {
    /// Every level as raw bytes in the texture's format, the way a GPU upload takes them.
    pub fn bytes(&self) -> Vec<u8> {
        self.levels
            .iter()
            .flat_map(|level| level.texels.iter())
            .flat_map(|&texel| self.format.encode_texel(texel))
            .collect()
    }

    /// The texel at a coordinate already known to be in bounds.
    pub fn texel(&self, level: usize, [x, y, z]: [u32; 3]) -> [f32; 4] {
        let level = &self.levels[level];
        let [width, height, _] = level.size;
        level.texels[((z * height + y) * width + x) as usize]
    }
}

impl Texture {
    /// The size of each mip level. Array layers do not shrink, 3D depth does.
    pub fn level_sizes(&self) -> Vec<[u32; 3]> {
        let [width, height, depth] = self.size;
        (0..self.mip_level_count)
            .map(|level| {
                let shrink = |n: u32| (n >> level).max(1);
                match self.dimension {
                    TextureDimension::D1 => [shrink(width), 1, 1],
                    TextureDimension::D2 => [shrink(width), shrink(height), depth],
                    TextureDimension::D3 => [shrink(width), shrink(height), shrink(depth)],
                }
            })
            .collect()
    }

    /// Decodes inline values or raw bytes. Path data resolves against `base_dir`.
    pub fn decode(&self, base_dir: Option<&Path>) -> Result<DecodedTexture, CaptureError> {
        let sizes = self.level_sizes();
        let count: usize = sizes.iter().map(|[w, h, d]| (w * h * d) as usize).sum();
        let format = self.format;
        let mut texels = match &self.data {
            TextureData::Inline(values) => {
                let components = format.components() as usize;
                if values.len() != count * components {
                    return Err(CaptureError::Data(format!(
                        "texture data has {} values but {count} texels of {components} components need {}",
                        values.len(),
                        count * components
                    )));
                }
                values.chunks_exact(components).map(widen).collect()
            }
            TextureData::Base64(text) => texels_from_bytes(&decode_base64(text)?, format, count)?,
            TextureData::Path(path) => texels_from_bytes(&read_file(path, base_dir)?, format, count)?,
        }
        .into_iter();

        let levels = sizes
            .into_iter()
            .map(|size| {
                let n = (size[0] * size[1] * size[2]) as usize;
                DecodedLevel {
                    size,
                    texels: texels.by_ref().take(n).collect(),
                }
            })
            .collect();
        Ok(DecodedTexture {
            dimension: self.dimension,
            format,
            levels,
        })
    }
}

impl BufferData {
    /// The raw bytes the shader sees, or `None` when the data is given as member values.
    pub fn bytes(&self, base_dir: Option<&Path>) -> Result<Option<Vec<u8>>, CaptureError> {
        match self {
            Self::Values(_) => Ok(None),
            Self::Base64(text) => decode_base64(text).map(Some),
            Self::Path(path) => read_file(path, base_dir).map(Some),
        }
    }
}

impl TextureFormat {
    /// Writes one texel, as the shader sees it, in this format's raw bytes.
    pub fn encode_texel(self, [r, g, b, a]: [f32; 4]) -> Vec<u8> {
        let unorm = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let floats = |values: &[f32]| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>()
        };
        match self {
            Self::R8Unorm => vec![unorm(r)],
            Self::R32Float => floats(&[r]),
            Self::Rg32Float => floats(&[r, g]),
            Self::Rgba8Unorm => vec![unorm(r), unorm(g), unorm(b), unorm(a)],
            Self::Rgba8UnormSrgb => vec![
                unorm(linear_to_srgb(r)),
                unorm(linear_to_srgb(g)),
                unorm(linear_to_srgb(b)),
                unorm(a),
            ],
            Self::Bgra8Unorm => vec![unorm(b), unorm(g), unorm(r), unorm(a)],
            Self::Rgba16Float => [r, g, b, a]
                .iter()
                .flat_map(|&v| f32_to_half(v).to_le_bytes())
                .collect(),
            Self::Rgba32Float => floats(&[r, g, b, a]),
        }
    }

    /// Reads one texel from raw bytes as the shader sees it.
    pub fn decode_texel(self, bytes: &[u8]) -> [f32; 4] {
        let unorm = |i: usize| f32::from(bytes[i]) / 255.0;
        let float =
            |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let half = |i: usize| half_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
        match self {
            Self::R8Unorm => [unorm(0), 0.0, 0.0, 1.0],
            Self::R32Float => [float(0), 0.0, 0.0, 1.0],
            Self::Rg32Float => [float(0), float(4), 0.0, 1.0],
            Self::Rgba8Unorm => [unorm(0), unorm(1), unorm(2), unorm(3)],
            Self::Rgba8UnormSrgb => [
                srgb_to_linear(unorm(0)),
                srgb_to_linear(unorm(1)),
                srgb_to_linear(unorm(2)),
                unorm(3),
            ],
            Self::Bgra8Unorm => [unorm(2), unorm(1), unorm(0), unorm(3)],
            Self::Rgba16Float => [half(0), half(2), half(4), half(6)],
            Self::Rgba32Float => [float(0), float(4), float(8), float(12)],
        }
    }
}

/// Decodes standard base64 with padding.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, CaptureError> {
    STANDARD
        .decode(text.trim())
        .map_err(|e| CaptureError::Data(format!("base64 data is malformed: {e}")))
}

/// Encodes bytes as standard base64 with padding.
pub fn encode_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Converts an IEEE half float to f32.
pub fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let out = match (exponent, mantissa) {
        (0, 0) => sign,
        // Subnormal halves are normal f32 values.
        (0, m) => {
            let shift = m.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((m << shift) & 0x3ff) << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, m) => sign | 0x7fc0_0000 | (m << 13),
        (e, m) => sign | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(out)
}

fn texels_from_bytes(
    bytes: &[u8],
    format: TextureFormat,
    count: usize,
) -> Result<Vec<[f32; 4]>, CaptureError> {
    let stride = format.bytes_per_texel() as usize;
    if bytes.len() != count * stride {
        return Err(CaptureError::Data(format!(
            "texture data is {} bytes but {count} texels of {stride} bytes need {}",
            bytes.len(),
            count * stride
        )));
    }
    Ok(bytes
        .chunks_exact(stride)
        .map(|chunk| format.decode_texel(chunk))
        .collect())
}

/// Pads one to four components the way a shader sees a narrower format.
fn widen(values: &[f32]) -> [f32; 4] {
    match *values {
        [r] => [r, 0.0, 0.0, 1.0],
        [r, g] => [r, g, 0.0, 1.0],
        [r, g, b] => [r, g, b, 1.0],
        [r, g, b, a, ..] => [r, g, b, a],
        [] => [0.0, 0.0, 0.0, 1.0],
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Converts f32 to an IEEE half, rounding to nearest even.
pub fn f32_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 0x1f {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let shift = (14 - half_exp) as u32;
        return sign | round_shift(mantissa | 0x80_0000, shift) as u16;
    }
    sign | round_shift(mantissa | ((half_exp as u32) << 23), 13) as u16
}

fn round_shift(value: u32, shift: u32) -> u32 {
    let half = 1 << (shift - 1);
    let rest = value & ((1 << shift) - 1);
    let mut out = value >> shift;
    if rest > half || (rest == half && out & 1 == 1) {
        out += 1;
    }
    out
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn read_file(path: &str, base_dir: Option<&Path>) -> Result<Vec<u8>, CaptureError> {
    let Some(base_dir) = base_dir else {
        return Err(CaptureError::Data(format!(
            "{path} is a file reference, which needs the capture's directory"
        )));
    };
    let path = base_dir.join(path);
    std::fs::read(&path).map_err(|source| CaptureError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use super::half_to_f32;

    #[test]
    fn reads_half_floats() {
        for (bits, value) in [
            (0x0000, 0.0),
            (0x3c00, 1.0),
            (0xc000, -2.0),
            (0x3555, 0.333_251_95),
            (0x0001, 5.960_464_5e-8),
            (0x7bff, 65504.0),
        ] {
            assert_eq!(half_to_f32(bits), value, "{bits:#06x}");
        }
        assert!(half_to_f32(0x7e00).is_nan());
        assert_eq!(half_to_f32(0xfc00), f32::NEG_INFINITY);
    }
}
