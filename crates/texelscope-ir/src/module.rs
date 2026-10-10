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
                let span = SourceSpan::from_naga(f.expressions.get_span(handle));
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
        SourceSpan::from_naga(f.expressions.get_span(handle))
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
