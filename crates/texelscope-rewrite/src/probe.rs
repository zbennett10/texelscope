//! Rewrites a shader so the GPU outputs one of its values at every pixel, instead of the
//! color, which shows that value over the whole frame in a single pass.

use serde::{Deserialize, Serialize};
use texelscope_ir::naga::{
    self, AddressSpace, Arena, Binding, Block, Expression, FunctionResult, GlobalVariable, Handle,
    Literal, Scalar, ScalarKind, Span, Statement, Type, TypeInner, VectorSize,
};
use texelscope_ir::{ExprRef, FunctionRef, Override, ShaderModule};

use crate::RewriteError;
use crate::patch;

/// What a probe outputs where a pixel never computed the value. WGSL evaluates a constant NaN
/// at compile time and rejects it, and the lowest float prints as a literal just past its
/// range, so a large negative number marks these pixels instead.
pub const NOT_COMPUTED: f32 = -1.0e38;

/// A shader rewritten to output one of its values instead of its color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// WGSL whose fragment entry point writes the value to location 0 as `vec4<f32>`, padded
    /// with zeros, and [`NOT_COMPUTED`] in every channel where a pixel never computed it.
    pub wgsl: String,
    /// How many channels hold the value.
    pub components: u32,
}

/// Rewrites the module so `entry_point` outputs `expr` at every pixel, with the overrides
/// applied first. The value must be a scalar or a vector.
pub fn probe(
    module: &ShaderModule,
    entry_point: &str,
    expr: ExprRef,
    overrides: &[Override],
) -> Result<Probe, RewriteError> {
    if overrides.is_empty() {
        return instrument(module, entry_point, expr);
    }
    let edits = patch::edits(module, overrides)?;
    let patched =
        ShaderModule::from_wgsl(patch::apply_edits(module.source(), &edits)).map_err(|e| {
            RewriteError::Emit {
                target: "patched WGSL",
                message: e.to_string(),
            }
        })?;
    let moved = patch::follow(module, &patched, &edits, expr).ok_or_else(|| {
        RewriteError::Unsupported(
            "a what-if replaced this value, so the GPU can't show it over the image".to_string(),
        )
    })?;
    instrument(&patched, entry_point, moved)
}

fn instrument(
    module: &ShaderModule,
    entry_point: &str,
    expr: ExprRef,
) -> Result<Probe, RewriteError> {
    let (scalar, components) = match module.type_of(expr) {
        Some(TypeInner::Scalar(s)) => (*s, 1),
        Some(TypeInner::Vector { size, scalar }) => (*scalar, *size as u32),
        _ => {
            return Err(RewriteError::Unsupported(
                "only numbers and vectors can be shown over the image".to_string(),
            ));
        }
    };
    let mut m = module.naga().clone();
    let entry = m
        .entry_points
        .iter()
        .position(|ep| ep.name == entry_point)
        .ok_or_else(|| RewriteError::NoEntryPoint(entry_point.to_string()))?;
    let vec4 = m.types.insert(
        Type {
            name: None,
            inner: TypeInner::Vector {
                size: VectorSize::Quad,
                scalar: Scalar::F32,
            },
        },
        Span::UNDEFINED,
    );
    let global = m.global_variables.append(
        GlobalVariable {
            name: Some("texelscope_probe".to_string()),
            space: AddressSpace::Private,
            binding: None,
            ty: vec4,
            init: None,
            memory_decorations: Default::default(),
        },
        Span::UNDEFINED,
    );

    // Store the value, as vec4<f32>, right after the shader computes it.
    let f = function_mut(&mut m, expr.function).ok_or_else(|| unknown(expr))?;
    let (target, _) = f
        .expressions
        .iter()
        .nth(expr.expr as usize)
        .ok_or_else(|| unknown(expr))?;
    let pointer = f
        .expressions
        .append(Expression::GlobalVariable(global), Span::UNDEFINED);
    let zero = f
        .expressions
        .append(Expression::Literal(Literal::F32(0.0)), Span::UNDEFINED);
    let first = f.expressions.len();
    let mut value = target;
    if scalar != Scalar::F32 {
        value = f.expressions.append(
            Expression::As {
                expr: value,
                kind: ScalarKind::Float,
                convert: Some(4),
            },
            Span::UNDEFINED,
        );
    }
    if components < 4 {
        let mut parts = vec![value];
        parts.extend(std::iter::repeat_n(zero, 4 - components as usize));
        value = f.expressions.append(
            Expression::Compose {
                ty: vec4,
                components: parts,
            },
            Span::UNDEFINED,
        );
    }
    let mut store = Block::new();
    if f.expressions.len() > first {
        store.push(
            Statement::Emit(f.expressions.range_from(first)),
            Span::UNDEFINED,
        );
    }
    store.push(Statement::Store { pointer, value }, Span::UNDEFINED);
    if let Expression::FunctionArgument(_) = f.expressions[target] {
        f.body.splice(0..0, store);
    } else if insert_after(&mut f.body, &f.expressions, target, store).is_err() {
        return Err(RewriteError::Unsupported(
            "this value is not computed in a place the GPU can record it".to_string(),
        ));
    }

    // Start every invocation at NOT_COMPUTED, so pixels that never compute the value show
    // none, and return the value instead of the color.
    let f = &mut m.entry_points[entry].function;
    let lowest = f.expressions.append(
        Expression::Literal(Literal::F32(NOT_COMPUTED)),
        Span::UNDEFINED,
    );
    let pointer = f
        .expressions
        .append(Expression::GlobalVariable(global), Span::UNDEFINED);
    let start = f.expressions.len();
    let filled = f.expressions.append(
        Expression::Splat {
            size: VectorSize::Quad,
            value: lowest,
        },
        Span::UNDEFINED,
    );
    let mut init = Block::new();
    init.push(
        Statement::Emit(f.expressions.range_from(start)),
        Span::UNDEFINED,
    );
    init.push(
        Statement::Store {
            pointer,
            value: filled,
        },
        Span::UNDEFINED,
    );
    f.body.splice(0..0, init);
    return_probe(&mut f.body, &mut f.expressions, pointer);
    f.result = Some(FunctionResult {
        ty: vec4,
        binding: Some(Binding::Location {
            location: 0,
            interpolation: None,
            sampling: None,
            blend_src: None,
            per_primitive: false,
        }),
    });

    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&m)
    .map_err(|e| RewriteError::Emit {
        target: "probed WGSL",
        message: e.into_inner().to_string(),
    })?;
    let wgsl = naga::back::wgsl::write_string(&m, &info, naga::back::wgsl::WriterFlags::empty())
        .map_err(|e| RewriteError::Emit {
            target: "probed WGSL",
            message: e.to_string(),
        })?;
    Ok(Probe { wgsl, components })
}

fn function_mut(m: &mut naga::Module, function: FunctionRef) -> Option<&mut naga::Function> {
    match function {
        FunctionRef::EntryPoint(i) => m
            .entry_points
            .get_mut(i as usize)
            .map(|ep| &mut ep.function),
        FunctionRef::Function(i) => m.functions.iter_mut().nth(i as usize).map(|(_, f)| f),
    }
}

fn unknown(expr: ExprRef) -> RewriteError {
    RewriteError::Unsupported(format!("there is no expression {} to show", expr.expr))
}

/// Puts `insert` right after the statement that computes `target`: inside its emit, which is
/// split in two, or after the call that returns it. Hands `insert` back when neither is found.
fn insert_after(
    block: &mut Block,
    exprs: &Arena<Expression>,
    target: Handle<Expression>,
    mut insert: Block,
) -> Result<(), Block> {
    let index = target.index() as u32;
    for i in 0..block.len() {
        match &block[i] {
            Statement::Emit(range) if range.index_range().contains(&index) => {
                let all = range.index_range();
                let mut replacement = Block::new();
                replacement.push(
                    Statement::Emit(naga::Range::from_index_range(all.start..index + 1, exprs)),
                    Span::UNDEFINED,
                );
                replacement.append(&mut insert);
                if index + 1 < all.end {
                    replacement.push(
                        Statement::Emit(naga::Range::from_index_range(index + 1..all.end, exprs)),
                        Span::UNDEFINED,
                    );
                }
                block.splice(i..i + 1, replacement);
                return Ok(());
            }
            Statement::Call {
                result: Some(result),
                ..
            } if *result == target => {
                block.splice(i + 1..i + 1, insert);
                return Ok(());
            }
            _ => {}
        }
        for nested in nested_blocks(&mut block[i]) {
            match insert_after(nested, exprs, target, insert) {
                Ok(()) => return Ok(()),
                Err(back) => insert = back,
            }
        }
    }
    Err(insert)
}

/// Makes every return in a block return the probe.
fn return_probe(block: &mut Block, exprs: &mut Arena<Expression>, pointer: Handle<Expression>) {
    let mut i = 0;
    while i < block.len() {
        if let Statement::Return { .. } = block[i] {
            let start = exprs.len();
            let load = exprs.append(Expression::Load { pointer }, Span::UNDEFINED);
            let mut replacement = Block::new();
            replacement.push(Statement::Emit(exprs.range_from(start)), Span::UNDEFINED);
            replacement.push(Statement::Return { value: Some(load) }, Span::UNDEFINED);
            block.splice(i..i + 1, replacement);
            i += 2;
            continue;
        }
        for nested in nested_blocks(&mut block[i]) {
            return_probe(nested, exprs, pointer);
        }
        i += 1;
    }
}

fn nested_blocks(statement: &mut Statement) -> Vec<&mut Block> {
    match statement {
        Statement::Block(inner) => vec![inner],
        Statement::If { accept, reject, .. } => vec![accept, reject],
        Statement::Loop {
            body, continuing, ..
        } => vec![body, continuing],
        Statement::Switch { cases, .. } => cases.iter_mut().map(|c| &mut c.body).collect(),
        _ => Vec::new(),
    }
}
