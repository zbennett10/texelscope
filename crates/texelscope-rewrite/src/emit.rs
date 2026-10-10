use texelscope_ir::ShaderModule;
use texelscope_ir::naga::back::{glsl, wgsl};
use texelscope_ir::naga::proc::BoundsCheckPolicies;

use crate::RewriteError;

/// Writes the module back out as WGSL.
pub fn emit_wgsl(module: &ShaderModule) -> Result<String, RewriteError> {
    wgsl::write_string(module.naga(), module.info(), wgsl::WriterFlags::empty()).map_err(|e| {
        RewriteError::Emit {
            target: "WGSL",
            message: e.to_string(),
        }
    })
}

/// Writes one entry point as GLSL ES 3.00 for WebGL2.
pub fn emit_glsl_es(module: &ShaderModule, entry_point: &str) -> Result<String, RewriteError> {
    let entry = module
        .naga()
        .entry_points
        .iter()
        .find(|ep| ep.name == entry_point)
        .ok_or_else(|| RewriteError::NoEntryPoint(entry_point.to_string()))?;
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..glsl::Options::default()
    };
    let pipeline = glsl::PipelineOptions {
        shader_stage: entry.stage,
        entry_point: entry_point.to_string(),
        multiview: None,
    };
    let emit_error = |e: glsl::Error| RewriteError::Emit {
        target: "GLSL ES",
        message: e.to_string(),
    };

    let mut out = String::new();
    glsl::Writer::new(
        &mut out,
        module.naga(),
        module.info(),
        &options,
        &pipeline,
        BoundsCheckPolicies::default(),
    )
    .map_err(emit_error)?
    .write()
    .map_err(emit_error)?;
    Ok(out)
}
