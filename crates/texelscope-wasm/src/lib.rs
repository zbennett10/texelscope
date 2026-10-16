//! Browser bindings for the Texelscope core. Results cross into JavaScript as JSON strings,
//! and region images as typed arrays.

use std::sync::Arc;

use texelscope_capture::{Rect, SourceRef};
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::{Override, ShaderModule};
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
    Ok(describe(&module, None).to_string())
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

/// One capture loaded for inspection. The shader can be swapped for an edited one while the
/// decoded data stays.
#[wasm_bindgen]
pub struct Session {
    data: Arc<CaptureData>,
    interp: Interpreter,
}

#[wasm_bindgen]
impl Session {
    /// Loads capture JSON whose shader source is inline.
    #[wasm_bindgen(constructor)]
    pub fn new(capture_json: &str) -> Result<Session, JsError> {
        let capture = texelscope_capture::parse(capture_json)?;
        let SourceRef::Inline(source) = &capture.shader.source else {
            return Err(JsError::new(
                "the capture's shader source must be inline in the browser",
            ));
        };
        let language = match capture.shader.language {
            texelscope_capture::Language::Wgsl => texelscope_ir::Language::Wgsl,
            texelscope_capture::Language::Glsl => texelscope_ir::Language::Glsl,
        };
        let module = ShaderModule::from_language(language, source.as_str())?;
        let data = Arc::new(CaptureData::new(capture, None)?);
        let interp = Interpreter::new(Arc::new(module), Arc::clone(&data))?;
        Ok(Session { data, interp })
    }

    /// The capture as JSON, with its data as given.
    #[wasm_bindgen(js_name = captureJson)]
    pub fn capture_json(&self) -> Result<String, JsError> {
        Ok(serde_json::to_string(self.data.capture())?)
    }

    /// The source being interpreted.
    pub fn source(&self) -> String {
        self.interp.module().source().to_string()
    }

    /// Swaps in an edited shader in the capture's language. Throws the diagnostic when it does
    /// not validate, and keeps the previous source.
    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(&mut self, source: &str) -> Result<String, JsError> {
        let module = ShaderModule::from_language(self.interp.module().language(), source)?;
        self.interp = Interpreter::new(Arc::new(module), Arc::clone(&self.data))?;
        Ok(self.describe())
    }

    /// Entry points, named values, resources and fragment inputs of the current source.
    pub fn describe(&self) -> String {
        let entry = &self.data.capture().shader.entry_point;
        describe(self.interp.module(), Some(entry)).to_string()
    }

    /// Whether the shader takes derivatives, so pixels run in quads.
    #[wasm_bindgen(js_name = usesQuads)]
    pub fn uses_quads(&self) -> bool {
        self.interp.uses_quads()
    }

    /// The trace of one pixel as JSON. `overrides` is a JSON array.
    #[wasm_bindgen(js_name = tracePixel)]
    pub fn trace_pixel(&self, x: u32, y: u32, overrides: &str) -> Result<String, JsError> {
        let trace = self
            .interp
            .trace_pixel([x, y], &parse_overrides(overrides)?)?;
        Ok(serde_json::to_string(&trace)?)
    }

    /// The shader rewritten to output one value at every pixel, as JSON with `source` and
    /// `components`. `expr` is a JSON expression reference and `overrides` a JSON array.
    #[wasm_bindgen(js_name = probeSource)]
    pub fn probe_source(&self, expr: &str, overrides: &str) -> Result<String, JsError> {
        let expr: texelscope_ir::ExprRef = serde_json::from_str(expr)?;
        let entry = &self.data.capture().shader.entry_point;
        let probe = texelscope_rewrite::probe(
            self.interp.module(),
            entry,
            expr,
            &parse_overrides(overrides)?,
        )?;
        Ok(serde_json::to_string(&probe)?)
    }

    /// What sets two pixels apart, as JSON.
    #[wasm_bindgen(js_name = comparePixels)]
    pub fn compare_pixels(
        &self,
        ax: u32,
        ay: u32,
        bx: u32,
        by: u32,
        overrides: &str,
    ) -> Result<String, JsError> {
        let comparison = texelscope_explain::compare_pixels(
            &self.interp,
            [ax, ay],
            [bx, by],
            &parse_overrides(overrides)?,
        )?;
        Ok(serde_json::to_string(&comparison)?)
    }

    /// The explanation of one pixel as JSON, with sensitivities.
    #[wasm_bindgen(js_name = explainPixel)]
    pub fn explain_pixel(&self, x: u32, y: u32, overrides: &str) -> Result<String, JsError> {
        let explanation =
            texelscope_explain::explain_pixel(&self.interp, [x, y], &parse_overrides(overrides)?)?;
        Ok(serde_json::to_string(&explanation)?)
    }

    /// Named values and decisions over a rectangle. Overrides apply inside `scope`, a JSON
    /// rectangle or `null` for everywhere.
    #[wasm_bindgen(js_name = inspectRegion)]
    pub fn inspect_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        overrides: &str,
        scope: &str,
    ) -> Result<RegionResult, JsError> {
        let scope: Option<Rect> = serde_json::from_str(scope)?;
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        let region = self
            .interp
            .inspect_region(rect, &parse_overrides(overrides)?, scope)?;
        let mut layers = Vec::with_capacity(region.layers.len() * region.colors.len());
        for layer in &region.layers {
            layers.extend_from_slice(&layer.data);
        }
        let mut branches = Vec::with_capacity(region.branches.len() * region.colors.len() / 4);
        for branch in &region.branches {
            branches.extend_from_slice(&branch.data);
        }
        Ok(RegionResult {
            meta: serde_json::to_string(&region)?,
            layers,
            branches,
            colors: region.colors,
        })
    }

    /// The bytes of a buffer binding laid out for the current shader, for a GPU upload.
    #[wasm_bindgen(js_name = bufferBytes)]
    pub fn buffer_bytes(&self, group: u32, binding: u32) -> Result<Vec<u8>, JsError> {
        Ok(self.interp.buffer_bytes(group, binding)?)
    }

    /// Every mip level of a texture binding as raw bytes in its format.
    #[wasm_bindgen(js_name = textureBytes)]
    pub fn texture_bytes(&self, group: u32, binding: u32) -> Result<Vec<u8>, JsError> {
        let texture = self.data.texture(group, binding).ok_or_else(|| {
            JsError::new(&format!(
                "no texture at @group({group}) @binding({binding})"
            ))
        })?;
        Ok(texture.bytes())
    }

    /// The source with overrides written into it, for replaying them on the GPU. GLSL comes
    /// back for the app's own context, with every loop limited.
    #[wasm_bindgen(js_name = patchedSource)]
    pub fn patched_source(&self, overrides: &str) -> Result<String, JsError> {
        Ok(texelscope_rewrite::gpu_source(
            self.interp.module(),
            &parse_overrides(overrides)?,
        )?)
    }
}

/// A region's layers. `layers` holds four floats per pixel for each layer in `meta`'s
/// order, `branches` one byte per pixel per branch, `colors` RGBA per pixel.
#[wasm_bindgen]
pub struct RegionResult {
    meta: String,
    layers: Vec<f32>,
    branches: Vec<u8>,
    colors: Vec<f32>,
}

#[wasm_bindgen]
impl RegionResult {
    #[wasm_bindgen(getter)]
    pub fn meta(&self) -> String {
        self.meta.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn layers(&self) -> Vec<f32> {
        self.layers.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn branches(&self) -> Vec<u8> {
        self.branches.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn colors(&self) -> Vec<f32> {
        self.colors.clone()
    }
}

fn parse_overrides(json: &str) -> Result<Vec<Override>, JsError> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(json)?)
}

fn describe(module: &ShaderModule, entry: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "entryPoints": module.entry_points(),
        "namedValues": module.named_values(),
        "resources": module.resources(),
        "stageInputs": entry.map(|e| module.stage_inputs(e)).unwrap_or_default(),
        "codeLines": module.code_lines(),
    })
}
