//! GLSL ES for the app's own WebGL2 context to run: the shader with what-ifs written in, or
//! rewritten to output one value. The rewrite edits the text, so every uniform, input and
//! output keeps its name and the app's bindings still fit. Every loop also gives up after
//! [`LOOP_LIMIT`] turns, so an edit or what-if that keeps a loop from ending can't hang the
//! app's GPU.

use texelscope_ir::naga::{Expression, Scalar, TypeInner};
use texelscope_ir::{ExprRef, Override, ShaderModule, glsl_outline};

use crate::probe::{NOT_COMPUTED, Probe};
use crate::{RewriteError, patch};

/// How many loop turns one pixel may take in all, across every loop, on the GPU.
pub const LOOP_LIMIT: u32 = 16384;

/// The shader with the overrides written in and its loops limited, for the app's context.
pub fn replay_glsl(module: &ShaderModule, overrides: &[Override]) -> Result<String, RewriteError> {
    let mut changes = Changes(loop_guards(module));
    let helpers = if changes.0.is_empty() {
        String::new()
    } else {
        loop_helpers()
    };
    changes.replace_all(patch::edits(module, overrides)?);
    let at = glsl_outline(module.source())
        .first_function
        .ok_or_else(|| RewriteError::Unsupported("the shader defines no function".to_string()))?;
    changes.insert(at, helpers);
    Ok(changes.apply(module.source()))
}

/// The shader rewritten so the app's GPU outputs `expr` at every pixel, as `vec4` floats at
/// location 0, with [`NOT_COMPUTED`] where a pixel never computes it.
pub(crate) fn probe_glsl(
    module: &ShaderModule,
    expr: ExprRef,
    overrides: &[Override],
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
    let source = module.source();
    let outline = glsl_outline(source);
    // Guards go first, so a guard and a flip of the same test wrap the flip.
    let mut changes = Changes(loop_guards(module));
    let helpers = if changes.0.is_empty() {
        String::new()
    } else {
        loop_helpers()
    };
    changes.replace_all(patch::edits(module, overrides)?);

    // An input holds one value all through `main`, so it is recorded where `main` starts.
    if let Some(name) = patch::glsl_input(module, expr) {
        let main = outline
            .main
            .last()
            .ok_or_else(|| RewriteError::Unsupported("the shader defines no main".to_string()))?;
        let brace = source[main.end..]
            .find('(')
            .and_then(|paren| patch::body_start(source, main.end + paren + 1))
            .ok_or_else(|| RewriteError::Unsupported("main has no body".to_string()))?;
        changes.insert(brace as usize + 1, format!(" texelscope_store({name});"));
        return finish_probe(module, changes, helpers, &outline, scalar, components, expr);
    }
    // Record the value as the shader computes it, through a function that hands it back.
    let span = module.span_of(expr).ok_or_else(|| {
        RewriteError::Unsupported("this value has no place in the source".to_string())
    })?;
    let code = module.text(span);
    let (f, h) = module.expression(expr).ok_or_else(|| unknown(expr))?;
    if let Expression::FunctionArgument(_) = f.expressions[h] {
        let brace = patch::body_start(source, span.end as usize).ok_or_else(|| {
            RewriteError::Unsupported("no function body follows this parameter".to_string())
        })?;
        changes.insert(brace as usize + 1, format!(" texelscope_store({code});"));
    } else if matches!(code.trim(), "++" | "--") {
        return Err(RewriteError::Unsupported(
            "an increment can't be shown over the image yet".to_string(),
        ));
    } else if patch::is_hidden_not(module, expr) {
        changes.wrap(
            span.start as usize,
            span.end as usize,
            "!texelscope_store(!(",
            "))",
        );
    } else {
        // A declaration's span covers its name, so only the value after `=` is wrapped.
        let start = match patch::assignment(code) {
            Some(op) if code[op..].starts_with('=') => {
                let after = op + 1;
                after + (code.len() - after - code[after..].trim_start().len())
            }
            _ => 0,
        };
        changes.wrap(
            span.start as usize + start,
            span.end as usize,
            "texelscope_store(",
            ")",
        );
    }

    finish_probe(module, changes, helpers, &outline, scalar, components, expr)
}

/// Turns the outputs into globals, runs `main` inside a new one that writes the recorded value,
/// and puts the helpers before the first function.
fn finish_probe(
    module: &ShaderModule,
    mut changes: Changes,
    mut helpers: String,
    outline: &texelscope_ir::GlslOutline,
    scalar: Scalar,
    components: u32,
    expr: ExprRef,
) -> Result<Probe, RewriteError> {
    let source = module.source();
    let first = outline
        .first_function
        .ok_or_else(|| RewriteError::Unsupported("the shader defines no function".to_string()))?;
    for qualifiers in &outline.outputs {
        changes.replace(qualifiers.start, qualifiers.end, String::new());
    }
    for name in &outline.main {
        changes.replace(name.start, name.end, "texelscope_main".to_string());
    }
    let param = module.glsl_type(module.type_of(expr).ok_or_else(|| unknown(expr))?);
    let widened = if scalar == Scalar::F32 {
        "v".to_string()
    } else if components == 1 {
        "float(v)".to_string()
    } else {
        format!("vec{components}(v)")
    };
    let padded = match components {
        4 => widened,
        n => format!("{widened}, {}", vec!["0.0"; 4 - n as usize].join(", ")),
    };
    // naga reads no precision on a parameter, so the value has the shader's default precision.
    helpers.push_str(&format!(
        "highp vec4 texelscope_probe; {param} texelscope_store({param} v) {{ texelscope_probe = vec4({padded}); return v; }} "
    ));
    changes.insert(first, helpers);
    changes.insert(
        source.len(),
        format!(
            "\nlayout(location = 0) out highp vec4 texelscope_out;\nvoid main() {{\n    texelscope_probe = vec4({NOT_COMPUTED:?});\n    texelscope_main();\n    texelscope_out = texelscope_probe;\n}}\n"
        ),
    );
    Ok(Probe {
        source: changes.apply(source),
        components,
    })
}

fn unknown(expr: ExprRef) -> RewriteError {
    RewriteError::Unsupported(format!("there is no expression {} to show", expr.expr))
}

/// The counter and the two tests a limited loop's condition goes through. Helpers share the
/// line of the first function, so compile errors keep the editor's line numbers.
fn loop_helpers() -> String {
    format!(
        "highp int texelscope_turns = 0; \
         bool texelscope_more(bool go) {{ texelscope_turns++; return go && texelscope_turns <= {LOOP_LIMIT}; }} \
         bool texelscope_stop(bool done) {{ texelscope_turns++; return done || texelscope_turns > {LOOP_LIMIT}; }} "
    )
}

/// A wrap around the test that ends each loop, so every loop counts its turns.
fn loop_guards(module: &ShaderModule) -> Vec<Change> {
    module
        .loop_exits()
        .into_iter()
        .filter_map(|exit| {
            let span = module.span_of(exit.condition)?;
            // A hidden `!` means the text is the condition to keep going.
            let ends_when_text_true =
                exit.exits_when != patch::is_hidden_not(module, exit.condition);
            let open = if ends_when_text_true {
                "texelscope_stop("
            } else {
                "texelscope_more("
            };
            Some(Change {
                start: span.start as usize,
                end: span.end as usize,
                kind: Kind::Wrap(open.to_string(), ")".to_string()),
            })
        })
        .collect()
}

/// Edits of a source by byte range. A replacement drops the changes inside it, and a wrap
/// keeps them inside its new text.
#[derive(Default)]
struct Changes(Vec<Change>);

struct Change {
    start: usize,
    end: usize,
    kind: Kind,
}

enum Kind {
    Replace(String),
    Wrap(String, String),
}

impl Changes {
    fn replace(&mut self, start: usize, end: usize, text: String) {
        self.0.push(Change {
            start,
            end,
            kind: Kind::Replace(text),
        });
    }

    fn replace_all(&mut self, edits: Vec<patch::Edit>) {
        for (span, text) in edits {
            self.replace(span.start as usize, span.end as usize, text);
        }
    }

    fn insert(&mut self, at: usize, text: String) {
        if !text.is_empty() {
            self.replace(at, at, text);
        }
    }

    fn wrap(&mut self, start: usize, end: usize, open: &str, close: &str) {
        self.0.push(Change {
            start,
            end,
            kind: Kind::Wrap(open.to_string(), close.to_string()),
        });
    }

    fn apply(mut self, source: &str) -> String {
        // Outer changes come first, by where they start and then the longest.
        self.0.sort_by_key(|c| (c.start, std::cmp::Reverse(c.end)));
        let mut out = String::with_capacity(source.len() + 512);
        write(source, &self.0, 0, source.len(), &mut out);
        out
    }
}

/// Writes `source[from..to]` with the changes inside that range. A change that overlaps one
/// written before it is left out.
fn write(source: &str, changes: &[Change], from: usize, to: usize, out: &mut String) {
    let mut at = from;
    let mut i = 0;
    while i < changes.len() {
        let c = &changes[i];
        i += 1;
        if c.start < at || c.end > to {
            continue;
        }
        out.push_str(&source[at..c.start]);
        // The changes after this one that sit inside it.
        let inner = changes[i..].iter().take_while(|d| d.start < c.end).count();
        match &c.kind {
            Kind::Replace(text) => out.push_str(text),
            Kind::Wrap(open, close) => {
                out.push_str(open);
                write(source, &changes[i..i + inner], c.start, c.end, out);
                out.push_str(close);
            }
        }
        i += inner;
        at = c.end;
    }
    out.push_str(&source[at..to]);
}
