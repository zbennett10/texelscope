//! A CPU render of a whole capture, written as a PNG.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use texelscope_interp::Interpreter;
use texelscope_ir::Override;

/// Shades every pixel and writes 8-bit RGBA. Discarded pixels are transparent.
pub fn write_png(interp: &Interpreter, overrides: &[Override], path: &Path) -> Result<()> {
    let target = interp.capture().target;
    let colors = interp.render(overrides)?;
    let bytes: Vec<u8> = colors
        .chunks_exact(4)
        .flat_map(|rgba| {
            if rgba[0].is_nan() {
                [0; 4]
            } else {
                std::array::from_fn::<u8, 4, _>(|i| (rgba[i].clamp(0.0, 1.0) * 255.0).round() as u8)
            }
        })
        .collect();
    let file =
        File::create(path).with_context(|| format!("could not create {}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), target.width, target.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
