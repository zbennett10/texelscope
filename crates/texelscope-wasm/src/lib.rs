//! Browser bindings for the Texelscope core. Results cross into JavaScript as JSON strings.

use texelscope_ir::ShaderModule;
use wasm_bindgen::prelude::*;

/// The version of the Texelscope core.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Parses and validates WGSL, then returns its entry points and named values as JSON.
#[wasm_bindgen(js_name = checkWgsl)]
pub fn check_wgsl(source: &str) -> Result<String, JsError> {
    let module = ShaderModule::from_wgsl(source)?;
    let report = serde_json::json!({
        "entryPoints": module.entry_points(),
        "namedValues": module.named_values(),
    });
    Ok(report.to_string())
}

/// Translates one entry point of a WGSL shader to GLSL ES 3.00.
#[wasm_bindgen(js_name = emitGlslEs)]
pub fn emit_glsl_es(source: &str, entry_point: &str) -> Result<String, JsError> {
    let module = ShaderModule::from_wgsl(source)?;
    Ok(texelscope_rewrite::emit_glsl_es(&module, entry_point)?)
}

/// Validates capture JSON and returns it normalized. Shader sources must be inline.
#[wasm_bindgen(js_name = parseCapture)]
pub fn parse_capture(json: &str) -> Result<String, JsError> {
    let capture = texelscope_capture::parse(json)?;
    Ok(serde_json::to_string(&capture)?)
}
