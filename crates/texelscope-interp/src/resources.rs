//! A capture's bindings, decoded once and matched to the globals a shader declares.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use texelscope_capture::{Capture, DecodedTexture, Resource, Sampler};
use texelscope_ir::naga::{self, AddressSpace, Handle, TypeInner};

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
        };
        for binding in &data.capture.bindings {
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
                    data.buffers.insert(key, source);
                }
                Resource::Texture(texture) => {
                    let decoded = texture.decode(base_dir).map_err(context)?;
                    data.textures.insert(key, Arc::new(decoded));
                }
                Resource::Sampler(sampler) => {
                    data.samplers.insert(key, *sampler);
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
                    match data.buffers.get(&key) {
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
                    TypeInner::Image { .. } => match data.textures.get(&key) {
                        Some(texture) => Global::Texture {
                            texture: Arc::clone(texture),
                            group,
                            binding,
                        },
                        None => missing(format!("the capture has no texture for {name}")),
                    },
                    TypeInner::Sampler { comparison: false } => match data.samplers.get(&key) {
                        Some(sampler) => Global::Sampler(*sampler),
                        None => missing(format!("the capture has no sampler for {name}")),
                    },
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
