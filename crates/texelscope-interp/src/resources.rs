//! A capture's bindings, decoded once and matched to the globals a shader declares.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use texelscope_capture::{Capture, DecodedTexture, FragmentInputs, Rect, Resource, Sampler};
use texelscope_ir::naga::{self, AddressSpace, Handle, TypeInner};
use texelscope_ir::{GlslLayout, GlslUniformKind};

use crate::val::Val;
use crate::{InterpError, layout};

/// A capture with its texture data decoded and its buffer bytes loaded. It does not depend
/// on the shader, so it survives edits to the source.
#[derive(Debug)]
pub struct CaptureData {
    capture: Capture,
    buffers: HashMap<(u32, u32), BufferSource>,
    textures: HashMap<(u32, u32), Arc<DecodedTexture>>,
    samplers: HashMap<(u32, u32), Sampler>,
    /// Bindings a WebGL capture gives by GLSL name instead of by number.
    named_buffers: HashMap<String, BufferSource>,
    named_textures: HashMap<String, Arc<DecodedTexture>>,
    named_samplers: HashMap<String, Sampler>,
    window: Option<Window>,
}

/// A capture's window inputs, decoded.
#[derive(Debug)]
pub(crate) struct Window {
    pub(crate) rect: Rect,
    /// Four floats per pixel, rows from the top.
    pub(crate) frag_coord: Vec<f32>,
    /// Each input's floats per pixel and values, by name.
    pub(crate) varyings: HashMap<String, (u32, Vec<f32>)>,
}

#[derive(Debug)]
enum BufferSource {
    Bytes(Vec<u8>),
    Values(BTreeMap<String, serde_json::Value>),
}

impl CaptureData {
    /// Decodes every binding. File references resolve against `base_dir`.
    pub fn new(capture: Capture, base_dir: Option<&Path>) -> Result<Self, InterpError> {
        let mut data = Self {
            capture,
            buffers: HashMap::new(),
            textures: HashMap::new(),
            samplers: HashMap::new(),
            named_buffers: HashMap::new(),
            named_textures: HashMap::new(),
            named_samplers: HashMap::new(),
            window: None,
        };
        if let FragmentInputs::Window {
            rect,
            frag_coord,
            varyings,
        } = &data.capture.inputs
        {
            let mut decoded = HashMap::new();
            for v in varyings {
                decoded.insert(v.name.clone(), (v.components, floats(&v.data)?));
            }
            data.window = Some(Window {
                rect: *rect,
                frag_coord: floats(frag_coord)?,
                varyings: decoded,
            });
        }
        for binding in &data.capture.bindings {
            let name = binding.name.clone();
            let key = (binding.group, binding.binding);
            let context = |e: texelscope_capture::CaptureError| {
                InterpError::BadData(format!(
                    "@group({}) @binding({}): {e}",
                    binding.group, binding.binding
                ))
            };
            match &binding.resource {
                Resource::Uniform { data: buffer } | Resource::Storage { data: buffer } => {
                    let source = match buffer.bytes(base_dir).map_err(context)? {
                        Some(bytes) => BufferSource::Bytes(bytes),
                        None => match buffer {
                            texelscope_capture::BufferData::Values(values) => {
                                BufferSource::Values(values.clone())
                            }
                            _ => unreachable!("only member values have no bytes"),
                        },
                    };
                    match name {
                        Some(name) => data.named_buffers.insert(name, source),
                        None => data.buffers.insert(key, source),
                    };
                }
                Resource::Texture(texture) => {
                    let decoded = Arc::new(texture.decode(base_dir).map_err(context)?);
                    match name {
                        Some(name) => data.named_textures.insert(name, decoded),
                        None => data.textures.insert(key, decoded),
                    };
                }
                Resource::Sampler(sampler) => {
                    match name {
                        Some(name) => data.named_samplers.insert(name, *sampler),
                        None => data.samplers.insert(key, *sampler),
                    };
                }
            }
        }
        Ok(data)
    }

    pub fn capture(&self) -> &Capture {
        &self.capture
    }

    /// The decoded texture at a binding.
    pub fn texture(&self, group: u32, binding: u32) -> Option<&DecodedTexture> {
        self.textures.get(&(group, binding)).map(Arc::as_ref)
    }

    pub(crate) fn window(&self) -> Option<&Window> {
        self.window.as_ref()
    }
}

/// Little-endian f32s from base64.
fn floats(base64: &str) -> Result<Vec<f32>, InterpError> {
    let bytes = texelscope_capture::decode_base64(base64)
        .map_err(|e| InterpError::BadData(format!("window inputs: {e}")))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// The GLSL name a binding of group 0 was given, for captures that bind by name.
fn glsl_name(
    glsl: Option<&GlslLayout>,
    group: u32,
    binding: u32,
    kinds: &[GlslUniformKind],
) -> Option<String> {
    let layout = glsl.filter(|_| group == 0)?;
    layout
        .uniforms
        .iter()
        .find(|u| u.binding == binding && kinds.contains(&u.kind))
        .map(|u| u.name.clone())
}

/// What one of the shader's globals resolves to.
#[derive(Debug)]
pub(crate) enum Global {
    Buffer {
        value: Val,
        group: u32,
        binding: u32,
        storage: bool,
    },
    Texture {
        texture: Arc<DecodedTexture>,
        group: u32,
        binding: u32,
    },
    Sampler(Sampler),
    /// Per-invocation memory, starting from this value.
    Private(Val),
    /// A binding the capture does not provide. Using it is an error.
    Missing {
        group: u32,
        binding: u32,
        reason: String,
    },
}

/// Resolves every global of `module` against the capture.
pub(crate) fn resolve(
    module: &naga::Module,
    data: &CaptureData,
    glsl: Option<&GlslLayout>,
    constant: &dyn Fn(Handle<naga::Expression>) -> Result<Val, InterpError>,
) -> Result<Vec<Global>, InterpError> {
    module
        .global_variables
        .iter()
        .map(|(_, var)| {
            let name = var.name.as_deref().unwrap_or("?");
            let Some(rb) = &var.binding else {
                let value = match var.init {
                    Some(init) => constant(init)?,
                    None => Val::zero(module, var.ty)?,
                };
                return Ok(Global::Private(value));
            };
            let (group, binding) = (rb.group, rb.binding);
            let key = (group, binding);
            let missing = |reason: String| Global::Missing {
                group,
                binding,
                reason,
            };
            Ok(match var.space {
                AddressSpace::Uniform | AddressSpace::Storage { .. } => {
                    let named = || {
                        let kinds = [GlslUniformKind::Value, GlslUniformKind::Block];
                        glsl_name(glsl, group, binding, &kinds)
                            .and_then(|n| data.named_buffers.get(&n))
                    };
                    match data.buffers.get(&key).or_else(named) {
                        None => missing(format!("the capture has no buffer for {name}")),
                        Some(source) => {
                            let value = match source {
                                BufferSource::Bytes(bytes) => {
                                    layout::read(module, var.ty, bytes, 0)
                                }
                                BufferSource::Values(values) => {
                                    buffer_from_values(module, var.ty, values, name)
                                }
                            };
                            match value {
                                Ok(value) => Global::Buffer {
                                    value,
                                    group,
                                    binding,
                                    storage: matches!(var.space, AddressSpace::Storage { .. }),
                                },
                                Err(e) => missing(e.to_string()),
                            }
                        }
                    }
                }
                AddressSpace::Handle => match &module.types[var.ty].inner {
                    TypeInner::Image { .. } => match data.textures.get(&key).or_else(|| {
                        glsl_name(glsl, group, binding, &[GlslUniformKind::Texture])
                            .and_then(|n| data.named_textures.get(&n))
                    }) {
                        Some(texture) => Global::Texture {
                            texture: Arc::clone(texture),
                            group,
                            binding,
                        },
                        None => missing(format!("the capture has no texture for {name}")),
                    },
                    TypeInner::Sampler { comparison: false } => {
                        match data.samplers.get(&key).or_else(|| {
                            glsl_name(glsl, group, binding, &[GlslUniformKind::Sampler])
                                .and_then(|n| data.named_samplers.get(&n))
                        }) {
                            Some(sampler) => Global::Sampler(*sampler),
                            None => missing(format!("the capture has no sampler for {name}")),
                        }
                    }
                    other => missing(format!(
                        "{name} is a {other:?}, which is not interpreted yet"
                    )),
                },
                other => missing(format!(
                    "{name} is in the {other:?} address space, which is not interpreted yet"
                )),
            })
        })
        .collect()
}

/// Member values for a struct, or the single value of a buffer that is not one.
fn buffer_from_values(
    module: &naga::Module,
    ty: Handle<naga::Type>,
    values: &BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Result<Val, InterpError> {
    if let TypeInner::Struct { .. } = module.types[ty].inner {
        let object: serde_json::Map<String, serde_json::Value> =
            values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        return layout::from_json(module, ty, &serde_json::Value::Object(object), name);
    }
    match values.values().next() {
        Some(value) if values.len() == 1 => layout::from_json(module, ty, value, name),
        _ => Err(InterpError::BadData(format!(
            "{name} is not a struct, so its values should hold exactly one entry"
        ))),
    }
}
