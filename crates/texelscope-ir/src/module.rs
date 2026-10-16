use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use serde::{Deserialize, Serialize};

use crate::glsl::GlslLayout;
use crate::ids::{ExprRef, FunctionRef, LineCol, SourceSpan};

/// The language a shader is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Wgsl,
    /// GLSL ES 3.00, as WebGL2 runs it.
    Glsl,
}

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

/// How many turns one invocation's loops may take in all, when a WebGL2 app runs an edit.
/// The GLSL it runs counts the turns, and so does the interpreter for GLSL, so an edit or
/// what-if that keeps a loop going ends the same way on both, without hanging the app's GPU.
pub const LOOP_LIMIT: u32 = 16384;

/// The test that ends a loop, which [`LOOP_LIMIT`] applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopExit {
    pub condition: ExprRef,
    /// Whether the loop ends when the condition is true.
    pub exits_when: bool,
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
    /// Where a GLSL shader's inputs and uniforms went, or `None` for WGSL.
    glsl: Option<GlslLayout>,
}

impl ShaderModule {
    /// Parses and validates a GLSL ES 3.00 fragment shader, as WebGL2 runs it. naga reads
    /// Vulkan GLSL 450, so the shader is rewritten first, and every span is moved back onto
    /// the original source, which is what [`ShaderModule::source`] returns.
    pub fn from_glsl(source: impl Into<String>) -> Result<Self, IrError> {
        let source = source.into();
        let converted = crate::glsl::convert(&source).map_err(IrError::Parse)?;
        let options = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
        let mut module = naga::front::glsl::Frontend::default()
            .parse(&options, &converted.text)
            .map_err(|e| IrError::Parse(e.emit_to_string(&converted.text)))?;
        let info = Validator::new(ValidationFlags::all(), Capabilities::default())
            .validate(&module)
            .map_err(|e| IrError::Validate(e.emit_to_string(&converted.text)))?;
        let map = |span: naga::Span| match span.to_range() {
            Some(r) => naga::Span::new(
                converted.original(r.start, false) as u32,
                converted.original(r.end, true) as u32,
            ),
            None => span,
        };
        remap_spans(&mut module, &map);
        cover_accessed(&mut module, &source);
        cover_operands(&mut module, &source);
        cover_parentheses(&mut module, &source);
        name_uniforms(&mut module, &converted.layout);
        Ok(Self {
            source,
            module,
            info,
            glsl: Some(converted.layout),
        })
    }

    /// Where a GLSL shader's inputs and uniforms went, by their names in WebGL.
    pub fn glsl_layout(&self) -> Option<&GlslLayout> {
        self.glsl.as_ref()
    }

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
            glsl: None,
        })
    }

    /// Parses a shader in its capture's language.
    pub fn from_language(language: Language, source: impl Into<String>) -> Result<Self, IrError> {
        match language {
            Language::Wgsl => Self::from_wgsl(source),
            Language::Glsl => Self::from_glsl(source),
        }
    }

    /// The language the source is written in.
    pub fn language(&self) -> Language {
        if self.glsl.is_some() {
            Language::Glsl
        } else {
            Language::Wgsl
        }
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
        let (info, _) = self.expression_info(expr)?;
        Some(info.ty.inner_with(&self.module.types))
    }

    /// Whether an expression has the same value at every pixel, because it comes only from
    /// constants and settings such as uniforms. naga's uniformity analysis decides, and it
    /// counts locals, helper parameters and anything from a per-pixel input as varying.
    pub fn is_uniform(&self, expr: ExprRef) -> bool {
        self.expression_info(expr)
            .is_some_and(|(info, _)| info.uniformity.non_uniform_result.is_none())
    }

    fn expression_info(
        &self,
        expr: ExprRef,
    ) -> Option<(&naga::valid::ExpressionInfo, naga::Handle<naga::Expression>)> {
        let (_, handle) = self.expression(expr)?;
        let info = match expr.function {
            FunctionRef::EntryPoint(i) => self.info.get_entry_point(i as usize),
            FunctionRef::Function(i) => {
                let (h, _) = self.module.functions.iter().nth(i as usize)?;
                &self.info[h]
            }
        };
        Some((&info[handle], handle))
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

    /// The GLSL spelling of a scalar, vector or matrix type.
    pub fn glsl_type(&self, inner: &naga::TypeInner) -> String {
        let prefix = |s: naga::Scalar| match s.kind {
            naga::ScalarKind::Bool => "b",
            naga::ScalarKind::Sint => "i",
            naga::ScalarKind::Uint => "u",
            _ => "",
        };
        match *inner {
            naga::TypeInner::Scalar(s) => match s.kind {
                naga::ScalarKind::Bool => "bool",
                naga::ScalarKind::Sint => "int",
                naga::ScalarKind::Uint => "uint",
                _ => "float",
            }
            .to_string(),
            naga::TypeInner::Vector { size, scalar } => {
                format!("{}vec{}", prefix(scalar), size as u8)
            }
            naga::TypeInner::Matrix { columns, rows, .. } if columns == rows => {
                format!("mat{}", columns as u8)
            }
            naga::TypeInner::Matrix { columns, rows, .. } => {
                format!("mat{}x{}", columns as u8, rows as u8)
            }
            ref other => format!("{other:?}"),
        }
    }

    /// The spelling of a scalar, vector or matrix type in the source's language.
    pub fn type_name(&self, inner: &naga::TypeInner) -> String {
        match self.language() {
            Language::Wgsl => self.wgsl_type(inner),
            Language::Glsl => self.glsl_type(inner),
        }
    }

    /// The test of each loop that [`LOOP_LIMIT`] applies to, for GLSL: the first `if` at the
    /// top of a loop's body that leaves it, or else its `break if`. Only tests with a place in
    /// the source count, since the GLSL that runs counts turns there.
    pub fn loop_exits(&self) -> Vec<LoopExit> {
        if self.language() != Language::Glsl {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (function, _, f) in self.functions() {
            loop_exits(self, function, &f.body, &mut out);
        }
        out
    }

    /// The 1-based line and column of a byte offset.
    pub fn line_col(&self, offset: u32) -> LineCol {
        let prefix = self.source.get(..offset as usize).unwrap_or(&self.source);
        let line = prefix.matches('\n').count() as u32 + 1;
        let column = prefix.rsplit('\n').next().map_or(0, |s| s.chars().count()) as u32 + 1;
        LineCol { line, column }
    }

    /// The lines, counted from 1, that compute a value a trace records when it runs: the
    /// emitted expressions and call results of every function. A viewer that dims the ones a
    /// trace never touched shows which code a pixel skipped.
    pub fn code_lines(&self) -> Vec<u32> {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(self.source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let line = |offset: u32| starts.partition_point(|&s| s <= offset as usize) as u32;
        let mut lines = std::collections::BTreeSet::new();
        for (function, _, f) in self.functions() {
            let mut handles = Vec::new();
            recorded_expressions(&f.body, &mut handles);
            for handle in handles {
                // A pointer into a variable is no value of its own, so no step records it.
                let expr = ExprRef {
                    function,
                    expr: handle.index() as u32,
                };
                if matches!(
                    self.type_of(expr),
                    Some(naga::TypeInner::Pointer { .. } | naga::TypeInner::ValuePointer { .. })
                ) {
                    continue;
                }
                if let Some(span) = SourceSpan::from_naga(f.expressions.get_span(handle)) {
                    lines.extend(line(span.start)..=line(span.end.max(span.start + 1) - 1));
                }
            }
        }
        lines.into_iter().collect()
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

fn loop_exits(
    module: &ShaderModule,
    function: FunctionRef,
    block: &naga::Block,
    out: &mut Vec<LoopExit>,
) {
    use naga::Statement as S;
    let leaves = |b: &naga::Block| b.iter().any(|s| matches!(s, S::Break | S::Return { .. }));
    for statement in block.iter() {
        match statement {
            S::Loop {
                body,
                continuing,
                break_if,
            } => {
                let test = body.iter().find_map(|s| match s {
                    S::If {
                        condition,
                        accept,
                        reject,
                    } if leaves(accept) || leaves(reject) => Some((*condition, leaves(accept))),
                    _ => None,
                });
                if let Some((condition, exits_when)) = test.or(break_if.map(|c| (c, true))) {
                    let condition = ExprRef {
                        function,
                        expr: condition.index() as u32,
                    };
                    if module.span_of(condition).is_some_and(|s| s.end > s.start) {
                        out.push(LoopExit {
                            condition,
                            exits_when,
                        });
                    }
                }
                loop_exits(module, function, body, out);
                loop_exits(module, function, continuing, out);
            }
            S::Block(inner) => loop_exits(module, function, inner, out),
            S::If { accept, reject, .. } => {
                loop_exits(module, function, accept, out);
                loop_exits(module, function, reject, out);
            }
            S::Switch { cases, .. } => {
                for case in cases {
                    loop_exits(module, function, &case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// The expressions of a block that run as steps: those it emits and the results of its calls.
fn recorded_expressions(block: &naga::Block, out: &mut Vec<naga::Handle<naga::Expression>>) {
    use naga::Statement as S;
    for statement in block.iter() {
        match statement {
            S::Emit(range) => out.extend(range.clone()),
            S::Call {
                result: Some(result),
                ..
            } => out.push(*result),
            S::Block(inner) => recorded_expressions(inner, out),
            S::If { accept, reject, .. } => {
                recorded_expressions(accept, out);
                recorded_expressions(reject, out);
            }
            S::Switch { cases, .. } => {
                for case in cases {
                    recorded_expressions(&case.body, out);
                }
            }
            S::Loop {
                body, continuing, ..
            } => {
                recorded_expressions(body, out);
                recorded_expressions(continuing, out);
            }
            _ => {}
        }
    }
}

/// Moves every expression and statement span through `map`.
fn remap_spans(module: &mut naga::Module, map: &dyn Fn(naga::Span) -> naga::Span) {
    let functions = module
        .functions
        .iter_mut()
        .map(|(_, f)| f)
        .chain(module.entry_points.iter_mut().map(|ep| &mut ep.function));
    for f in functions {
        let old = std::mem::take(&mut f.expressions);
        for (handle, expression) in old.iter() {
            f.expressions
                .append(expression.clone(), map(old.get_span(handle)));
        }
        let old = std::mem::take(&mut f.local_variables);
        for (handle, local) in old.iter() {
            f.local_variables
                .append(local.clone(), map(old.get_span(handle)));
        }
        remap_block(&mut f.body, map);
    }
}

/// Takes in the parentheses a span opens or closes without the other half. naga's GLSL front
/// end ends `a * (b + c)` at `c`, which leaves the text a pin would replace unbalanced.
fn cover_parentheses(module: &mut naga::Module, source: &str) {
    let bytes = source.as_bytes();
    let functions = module
        .functions
        .iter_mut()
        .map(|(_, f)| f)
        .chain(module.entry_points.iter_mut().map(|ep| &mut ep.function));
    for f in functions {
        let old = std::mem::take(&mut f.expressions);
        for (handle, expression) in old.iter() {
            let span = match old.get_span(handle).to_range() {
                Some(r) => {
                    let r = balanced(bytes, r);
                    naga::Span::new(r.start as u32, r.end as u32)
                }
                None => naga::Span::UNDEFINED,
            };
            f.expressions.append(expression.clone(), span);
        }
    }
}

/// A range widened over the brackets just outside it that close or open the ones inside.
fn balanced(bytes: &[u8], range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let Some(text) = bytes.get(range.clone()) else {
        return range;
    };
    let (mut start, mut end) = (range.start, range.end);
    let mut depth = 0i32;
    let mut lowest = 0i32;
    for &b in text {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        lowest = lowest.min(depth);
    }
    // Closers the text needs on its right, and openers it needs on its left.
    let mut closers = depth - lowest;
    let mut openers = -lowest;
    let (old_start, old_end) = (start, end);
    while closers > 0 {
        while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
            end += 1;
        }
        if bytes.get(end) != Some(&b')') {
            return range;
        }
        end += 1;
        closers -= 1;
    }
    while openers > 0 {
        while start > 0 && bytes[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        if start == 0 || bytes[start - 1] != b'(' {
            return old_start..old_end;
        }
        start -= 1;
        openers -= 1;
    }
    start..end
}

/// Starts each expression's span at what a `.` or `[` it starts with applies to. naga's GLSL
/// front end starts a member or index access there, so `v.x > 0.5 ? a : b` would lose its `v`.
fn cover_accessed(module: &mut naga::Module, source: &str) {
    let functions = module
        .functions
        .iter_mut()
        .map(|(_, f)| f)
        .chain(module.entry_points.iter_mut().map(|ep| &mut ep.function));
    for f in functions {
        let old = std::mem::take(&mut f.expressions);
        for (handle, expression) in old.iter() {
            let span = match old.get_span(handle).to_range() {
                Some(r) => naga::Span::new(
                    accessed_start(source.as_bytes(), r.start) as u32,
                    r.end as u32,
                ),
                None => naga::Span::UNDEFINED,
            };
            f.expressions.append(expression.clone(), span);
        }
    }
}

/// Where the access starting at `start` begins, if a `.` or `[` is there: back over the
/// name, call or indexed value before it, and any accesses before that.
fn accessed_start(bytes: &[u8], mut start: usize) -> usize {
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let skip_space = |mut at: usize| {
        while at > 0 && bytes[at - 1].is_ascii_whitespace() {
            at -= 1;
        }
        at
    };
    while matches!(bytes.get(start), Some(b'.' | b'[')) {
        let mut at = skip_space(start);
        match at.checked_sub(1).map(|i| bytes[i]) {
            Some(b')' | b']') => {
                let Some(open) = opener(bytes, at - 1) else {
                    break;
                };
                at = open;
                while at > 0 && ident(bytes[at - 1]) {
                    at -= 1;
                }
            }
            Some(b) if ident(b) => {
                while at > 0 && ident(bytes[at - 1]) {
                    at -= 1;
                }
            }
            _ => break,
        }
        // `a.b` reached from `.c` goes on through the `.` before `b`.
        let before = skip_space(at);
        start = if before > 0 && bytes[before - 1] == b'.' {
            before - 1
        } else {
            at
        };
    }
    start
}

/// The `(` or `[` that the bracket at `close` closes.
fn opener(bytes: &[u8], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for at in (0..=close).rev() {
        match bytes[at] {
            b')' | b']' => depth += 1,
            b'(' | b'[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Widens each expression's span to take in operands on its own line. naga's GLSL front end
/// spans `uv.x * k` from `.x`, leaving out the `uv` the access starts from.
fn cover_operands(module: &mut naga::Module, source: &str) {
    let line_start = |offset: usize| {
        source[..offset.min(source.len())]
            .rfind('\n')
            .map_or(0, |i| i + 1)
    };
    let functions = module
        .functions
        .iter_mut()
        .map(|(_, f)| f)
        .chain(module.entry_points.iter_mut().map(|ep| &mut ep.function));
    for f in functions {
        let old = std::mem::take(&mut f.expressions);
        let mut spans: Vec<Option<std::ops::Range<usize>>> = Vec::with_capacity(old.len());
        for (handle, expression) in old.iter() {
            let own = old.get_span(handle).to_range();
            let mut span = own.clone();
            if let Some(own) = own {
                let first = line_start(own.start);
                for operand in value_operands(expression) {
                    let Some(Some(o)) = spans.get(operand.index()) else {
                        continue;
                    };
                    if o.start >= first && o.end <= own.end {
                        let s = span.get_or_insert(own.clone());
                        s.start = s.start.min(o.start);
                    }
                }
            } else if let Some(Some(first)) =
                call_argument(expression).map(|a| spans.get(a.index()))
            {
                // naga leaves builtin calls such as `step(a, b)` unspanned.
                span = first
                    .as_ref()
                    .and_then(|first| enclosing_call(source, first.start));
            }
            spans.push(span.clone());
            let span = span.map_or(naga::Span::UNDEFINED, |r| {
                naga::Span::new(r.start as u32, r.end as u32)
            });
            f.expressions.append(expression.clone(), span);
        }
    }
}

/// The first argument of a builtin call, for the expressions naga makes from calls.
fn call_argument(expression: &naga::Expression) -> Option<naga::Handle<naga::Expression>> {
    match *expression {
        naga::Expression::Math { arg, .. } => Some(arg),
        naga::Expression::Relational { argument, .. } => Some(argument),
        _ => None,
    }
}

/// The span of `name(...)` around a call's first argument, which starts at `first`.
fn enclosing_call(source: &str, first: usize) -> Option<std::ops::Range<usize>> {
    let before = source
        .get(..first)?
        .trim_end()
        .strip_suffix('(')?
        .trim_end();
    let name = before
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    if name == 0 {
        return None;
    }
    let start = before.len() - name;
    let open = start + source[start..].find('(')?;
    let mut depth = 0;
    for (i, c) in source[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start..open + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The operands of an expression that compute values where it is used, leaving out pointers
/// to variables, which naga spans by their declarations.
fn value_operands(expression: &naga::Expression) -> Vec<naga::Handle<naga::Expression>> {
    use naga::Expression as E;
    match *expression {
        E::AccessIndex { base, .. } => vec![base],
        E::Access { base, index } => vec![base, index],
        E::Swizzle { vector, .. } => vec![vector],
        E::Binary { left, right, .. } => vec![left, right],
        E::Unary { expr, .. } | E::As { expr, .. } | E::Derivative { expr, .. } => vec![expr],
        E::Select {
            condition,
            accept,
            reject,
        } => vec![condition, accept, reject],
        E::Math {
            arg,
            arg1,
            arg2,
            arg3,
            ..
        } => [Some(arg), arg1, arg2, arg3]
            .into_iter()
            .flatten()
            .collect(),
        E::Relational { argument, .. } => vec![argument],
        E::Splat { value, .. } => vec![value],
        E::Compose { ref components, .. } => components.clone(),
        _ => Vec::new(),
    }
}

/// Names the global of each uniform the rewrite wrapped in a block after the uniform, and an
/// unnamed block's global after the block, so resources read as the shader declares them.
fn name_uniforms(module: &mut naga::Module, layout: &GlslLayout) {
    for (_, global) in module.global_variables.iter_mut() {
        let Some(binding) = &global.binding else {
            continue;
        };
        if global.name.is_some() || binding.group != 0 {
            continue;
        }
        if let Some(u) = layout
            .uniforms
            .iter()
            .find(|u| u.binding == binding.binding)
        {
            global.name = Some(u.name.clone());
        }
    }
}

fn remap_block(block: &mut naga::Block, map: &dyn Fn(naga::Span) -> naga::Span) {
    for (statement, span) in block.span_iter_mut() {
        if let Some(span) = span {
            *span = map(*span);
        }
        match statement {
            naga::Statement::Block(inner) => remap_block(inner, map),
            naga::Statement::If { accept, reject, .. } => {
                remap_block(accept, map);
                remap_block(reject, map);
            }
            naga::Statement::Loop {
                body, continuing, ..
            } => {
                remap_block(body, map);
                remap_block(continuing, map);
            }
            naga::Statement::Switch { cases, .. } => {
                for case in cases {
                    remap_block(&mut case.body, map);
                }
            }
            _ => {}
        }
    }
}
