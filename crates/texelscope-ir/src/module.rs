use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use serde::{Deserialize, Serialize};

use crate::ids::{ExprRef, FunctionRef, LineCol, SourceSpan};

/// Why a shader could not be loaded. The message is naga's rendered diagnostic.
#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Validate(String),
}

/// The pipeline stage of an entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
    Other,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Fragment => "fragment",
            Self::Compute => "compute",
            Self::Other => "other",
        }
    }
}

impl From<naga::ShaderStage> for Stage {
    fn from(stage: naga::ShaderStage) -> Self {
        match stage {
            naga::ShaderStage::Vertex => Self::Vertex,
            naga::ShaderStage::Fragment => Self::Fragment,
            naga::ShaderStage::Compute => Self::Compute,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPointInfo {
    pub function: FunctionRef,
    pub name: String,
    pub stage: Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NamedValueKind {
    Argument,
    Let,
    Var,
}

/// An argument, `let` or `var` the source names. Explanations and the UI label values with
/// these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedValue {
    pub function: FunctionRef,
    pub function_name: String,
    pub name: String,
    pub kind: NamedValueKind,
    /// The expression an argument or `let` binds. A `var` has none.
    pub expr: Option<ExprRef>,
    pub span: Option<SourceSpan>,
    pub at: Option<LineCol>,
}

/// A resource the shader declares with `@group` and `@binding`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub kind: ResourceKind,
}

/// What a resource binding holds, in the terms of a WebGPU bind group layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResourceKind {
    Uniform,
    Storage {
        read_only: bool,
    },
    Texture {
        /// A GPUTextureViewDimension such as `2d` or `2d-array`.
        view_dimension: String,
        /// `float`, `sint`, `uint` or `depth`.
        sample_type: String,
        multisampled: bool,
    },
    StorageTexture {
        view_dimension: String,
    },
    Sampler {
        comparison: bool,
    },
    Other,
}

/// A user-defined input of a fragment entry point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageInput {
    pub location: u32,
    /// The WGSL type, such as `vec2<f32>`.
    pub ty: String,
    /// `perspective`, `linear` or `flat`, when the shader says.
    pub interpolation: Option<String>,
    /// `center`, `centroid` or `sample`, when the shader says.
    pub sampling: Option<String>,
}

/// A parsed and validated WGSL module, kept with its source.
#[derive(Debug)]
pub struct ShaderModule {
    source: String,
    module: naga::Module,
    info: ModuleInfo,
}

impl ShaderModule {
    /// Parses and validates WGSL with WebGPU's default capabilities.
    pub fn from_wgsl(source: impl Into<String>) -> Result<Self, IrError> {
        let source = source.into();
        let module = naga::front::wgsl::parse_str(&source)
            .map_err(|e| IrError::Parse(e.emit_to_string(&source)))?;
        let info = Validator::new(ValidationFlags::all(), Capabilities::default())
            .validate(&module)
            .map_err(|e| IrError::Validate(e.emit_to_string(&source)))?;
        Ok(Self {
            source,
            module,
            info,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn naga(&self) -> &naga::Module {
        &self.module
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.info
    }

    pub fn entry_points(&self) -> Vec<EntryPointInfo> {
        self.module
            .entry_points
            .iter()
            .enumerate()
            .map(|(i, ep)| EntryPointInfo {
                function: FunctionRef::EntryPoint(i as u32),
                name: ep.name.clone(),
                stage: ep.stage.into(),
            })
            .collect()
    }

    pub fn entry_point(&self, name: &str) -> Option<EntryPointInfo> {
        self.entry_points().into_iter().find(|ep| ep.name == name)
    }

    /// The naga function a reference points to, with its name.
    pub fn function(&self, function: FunctionRef) -> Option<(&str, &naga::Function)> {
        match function {
            FunctionRef::EntryPoint(i) => self
                .module
                .entry_points
                .get(i as usize)
                .map(|ep| (ep.name.as_str(), &ep.function)),
            FunctionRef::Function(i) => self
                .module
                .functions
                .iter()
                .nth(i as usize)
                .map(|(_, f)| (f.name.as_deref().unwrap_or(""), f)),
        }
    }

    /// Every named argument, `let` and `var`, in source order within each function.
    pub fn named_values(&self) -> Vec<NamedValue> {
        let mut values = Vec::new();
        for (function, function_name, f) in self.functions() {
            for (&handle, name) in f.named_expressions.iter() {
                let span =
                    SourceSpan::from_naga(f.expressions.get_span(handle)).map(|s| self.widen(s));
                // naga names argument expressions too, so tell them apart from lets.
                let kind = match f.expressions[handle] {
                    naga::Expression::FunctionArgument(_) => NamedValueKind::Argument,
                    _ => NamedValueKind::Let,
                };
                values.push(NamedValue {
                    function,
                    function_name: function_name.to_string(),
                    name: name.clone(),
                    kind,
                    expr: Some(ExprRef {
                        function,
                        expr: handle.index() as u32,
                    }),
                    span,
                    at: span.map(|s| self.line_col(s.start)),
                });
            }
            for (handle, var) in f.local_variables.iter() {
                let Some(name) = &var.name else { continue };
                let span = SourceSpan::from_naga(f.local_variables.get_span(handle));
                values.push(NamedValue {
                    function,
                    function_name: function_name.to_string(),
                    name: name.clone(),
                    kind: NamedValueKind::Var,
                    expr: None,
                    span,
                    at: span.map(|s| self.line_col(s.start)),
                });
            }
        }
        values.sort_by_key(|v| (v.function, v.span.map(|s| s.start)));
        values
    }

    /// Where an expression came from in the source, if naga kept a span for it.
    pub fn span_of(&self, expr: ExprRef) -> Option<SourceSpan> {
        let (_, f) = self.function(expr.function)?;
        let (handle, _) = f.expressions.iter().nth(expr.expr as usize)?;
        SourceSpan::from_naga(f.expressions.get_span(handle)).map(|s| self.widen(s))
    }

    /// Extends a span that ends at a function name to cover the call's arguments, since naga
    /// spans a builtin call by its name alone.
    pub fn widen(&self, span: SourceSpan) -> SourceSpan {
        let bytes = self.source.as_bytes();
        let mut at = span.end as usize;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let named = self
            .text(span)
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !named || bytes.get(at) != Some(&b'(') {
            return span;
        }
        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate().skip(at) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return SourceSpan {
                            start: span.start,
                            end: i as u32 + 1,
                        };
                    }
                }
                _ => {}
            }
        }
        span
    }

    /// The source text a span covers.
    pub fn text(&self, span: SourceSpan) -> &str {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }

    /// Every resource the module declares, ordered by group and binding.
    pub fn resources(&self) -> Vec<ResourceInfo> {
        let mut resources: Vec<ResourceInfo> = self
            .module
            .global_variables
            .iter()
            .filter_map(|(_, var)| {
                let binding = var.binding.as_ref()?;
                Some(ResourceInfo {
                    group: binding.group,
                    binding: binding.binding,
                    name: var.name.clone().unwrap_or_default(),
                    kind: self.resource_kind(var),
                })
            })
            .collect();
        resources.sort_by_key(|r| (r.group, r.binding));
        resources
    }

    fn resource_kind(&self, var: &naga::GlobalVariable) -> ResourceKind {
        use naga::{
            AddressSpace, ImageClass, ImageDimension, ScalarKind, StorageAccess, TypeInner,
        };
        let view = |dim: ImageDimension, arrayed: bool| {
            let base = match dim {
                ImageDimension::D1 => "1d",
                ImageDimension::D2 => "2d",
                ImageDimension::D3 => "3d",
                ImageDimension::Cube => "cube",
            };
            if arrayed {
                format!("{base}-array")
            } else {
                base.to_string()
            }
        };
        match (var.space, &self.module.types[var.ty].inner) {
            (AddressSpace::Uniform, _) => ResourceKind::Uniform,
            (AddressSpace::Storage { access }, _) => ResourceKind::Storage {
                read_only: !access.contains(StorageAccess::STORE),
            },
            (
                _,
                TypeInner::Image {
                    dim,
                    arrayed,
                    class,
                },
            ) => match *class {
                ImageClass::Sampled { kind, multi } => ResourceKind::Texture {
                    view_dimension: view(*dim, *arrayed),
                    sample_type: match kind {
                        ScalarKind::Sint => "sint",
                        ScalarKind::Uint => "uint",
                        _ => "float",
                    }
                    .to_string(),
                    multisampled: multi,
                },
                ImageClass::Depth { multi } => ResourceKind::Texture {
                    view_dimension: view(*dim, *arrayed),
                    sample_type: "depth".to_string(),
                    multisampled: multi,
                },
                ImageClass::Storage { .. } => ResourceKind::StorageTexture {
                    view_dimension: view(*dim, *arrayed),
                },
                ImageClass::External => ResourceKind::Other,
            },
            (_, TypeInner::Sampler { comparison }) => ResourceKind::Sampler {
                comparison: *comparison,
            },
            _ => ResourceKind::Other,
        }
    }

    /// The function an expression belongs to and its handle there.
    pub fn expression(
        &self,
        expr: ExprRef,
    ) -> Option<(&naga::Function, naga::Handle<naga::Expression>)> {
        let (_, f) = self.function(expr.function)?;
        let (handle, _) = f.expressions.iter().nth(expr.expr as usize)?;
        Some((f, handle))
    }

    /// The type naga inferred for an expression.
    pub fn type_of(&self, expr: ExprRef) -> Option<&naga::TypeInner> {
        let (_, handle) = self.expression(expr)?;
        let info = match expr.function {
            FunctionRef::EntryPoint(i) => self.info.get_entry_point(i as usize),
            FunctionRef::Function(i) => {
                let (h, _) = self.module.functions.iter().nth(i as usize)?;
                &self.info[h]
            }
        };
        Some(info[handle].ty.inner_with(&self.module.types))
    }

    /// The location inputs of a fragment entry point, for building a vertex stage that
    /// feeds it.
    pub fn stage_inputs(&self, entry_point: &str) -> Vec<StageInput> {
        let Some(ep) = self
            .module
            .entry_points
            .iter()
            .find(|ep| ep.name == entry_point)
        else {
            return Vec::new();
        };
        let mut inputs = Vec::new();
        let mut add = |binding: &Option<naga::Binding>, ty: naga::Handle<naga::Type>| {
            if let Some(naga::Binding::Location {
                location,
                interpolation,
                sampling,
                ..
            }) = binding
            {
                inputs.push(StageInput {
                    location: *location,
                    ty: self.wgsl_type(&self.module.types[ty].inner),
                    interpolation: interpolation.map(|i| format!("{i:?}").to_lowercase()),
                    sampling: sampling.map(|s| format!("{s:?}").to_lowercase()),
                });
            }
        };
        for arg in &ep.function.arguments {
            match &self.module.types[arg.ty].inner {
                naga::TypeInner::Struct { members, .. } if arg.binding.is_none() => {
                    for m in members {
                        add(&m.binding, m.ty);
                    }
                }
                _ => add(&arg.binding, arg.ty),
            }
        }
        inputs.sort_by_key(|i| i.location);
        inputs
    }

    /// The WGSL spelling of a scalar, vector or matrix type.
    pub fn wgsl_type(&self, inner: &naga::TypeInner) -> String {
        let scalar = |s: naga::Scalar| match (s.kind, s.width) {
            (naga::ScalarKind::Bool, _) => "bool".to_string(),
            (naga::ScalarKind::Sint, w) => format!("i{}", u32::from(w) * 8),
            (naga::ScalarKind::Uint, w) => format!("u{}", u32::from(w) * 8),
            (_, w) => format!("f{}", u32::from(w) * 8),
        };
        match *inner {
            naga::TypeInner::Scalar(s) => scalar(s),
            naga::TypeInner::Vector { size, scalar: s } => {
                format!("vec{}<{}>", size as u8, scalar(s))
            }
            naga::TypeInner::Matrix {
                columns,
                rows,
                scalar: s,
            } => {
                format!("mat{}x{}<{}>", columns as u8, rows as u8, scalar(s))
            }
            ref other => format!("{other:?}"),
        }
    }

    /// The 1-based line and column of a byte offset.
    pub fn line_col(&self, offset: u32) -> LineCol {
        let prefix = self.source.get(..offset as usize).unwrap_or(&self.source);
        let line = prefix.matches('\n').count() as u32 + 1;
        let column = prefix.rsplit('\n').next().map_or(0, |s| s.chars().count()) as u32 + 1;
        LineCol { line, column }
    }

    fn functions(&self) -> impl Iterator<Item = (FunctionRef, &str, &naga::Function)> {
        let helpers = self.module.functions.iter().map(|(handle, f)| {
            let name = f.name.as_deref().unwrap_or("");
            (FunctionRef::Function(handle.index() as u32), name, f)
        });
        let entries = self.module.entry_points.iter().enumerate().map(|(i, ep)| {
            (
                FunctionRef::EntryPoint(i as u32),
                ep.name.as_str(),
                &ep.function,
            )
        });
        helpers.chain(entries)
    }
}
