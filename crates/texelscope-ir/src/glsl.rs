//! GLSL ES 3.00 fragment shaders, as WebGL2 runs them, rewritten as the Vulkan GLSL 450 that
//! naga reads. The rewrite keeps every line where it was and records each edit, so spans in
//! naga's module map back to the original source.
//!
//! What changes: the version line, a location on every input and output, a binding on every
//! uniform, a block around each uniform that isn't in one, and each `sampler2D` uniform split
//! into a texture and a sampler, with its uses written as `sampler2D(texture, sampler)`.

use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Where the rewrite put each input and uniform, by the names WebGL reports for them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlslLayout {
    /// Fragment inputs by name, with the location each was given.
    pub inputs: Vec<GlslInput>,
    pub uniforms: Vec<GlslUniform>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlslInput {
    pub name: String,
    pub location: u32,
}

/// One uniform, uniform block, texture or sampler and the binding it was given, in group 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlslUniform {
    /// The uniform's name, or a block's name, as WebGL reports it.
    pub name: String,
    pub kind: GlslUniformKind,
    pub binding: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlslUniformKind {
    /// A uniform outside a block, now the only member of a block of its own.
    Value,
    /// A uniform block, laid out with std140.
    Block,
    /// The texture half of a sampler uniform.
    Texture,
    /// The sampler half of a sampler uniform.
    Sampler,
}

/// The rewritten source and how to find the original behind any offset in it.
pub(crate) struct Converted {
    pub(crate) text: String,
    pub(crate) layout: GlslLayout,
    /// Each edit's range in the original and in the rewritten text, in order.
    edits: Vec<(Range<usize>, Range<usize>)>,
}

impl Converted {
    /// The original offset behind an offset of the rewritten text. An offset inside rewritten
    /// text maps to the start of what it replaced, or to its end when `end` is set.
    pub(crate) fn original(&self, offset: usize, end: bool) -> usize {
        let mut shift: isize = 0;
        for (original, converted) in &self.edits {
            if offset < converted.start {
                break;
            }
            if offset < converted.end || (end && offset == converted.end) {
                return if end && offset > converted.start {
                    original.end
                } else {
                    original.start
                };
            }
            shift = original.end as isize - converted.end as isize;
        }
        (offset as isize + shift).max(0) as usize
    }
}

/// A token of the source, by its byte range.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Ident,
    Number,
    Punct,
    /// A whole preprocessor line.
    Directive,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    range: (usize, usize),
}

fn tokens(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        let start = i;
        let kind = if b == b'#' && line_start {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            Kind::Directive
        } else if b.is_ascii_alphabetic() || b == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Kind::Ident
        } else if b.is_ascii_digit()
            || (b == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            Kind::Number
        } else {
            i += 1;
            Kind::Punct
        };
        line_start = false;
        out.push(Token {
            kind,
            range: (start, i),
        });
    }
    out
}

/// Sampler types and the texture, sampler and constructor each splits into.
fn sampler_parts(ty: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match ty {
        "sampler2D" => ("texture2D", "sampler", "sampler2D"),
        "isampler2D" => ("itexture2D", "sampler", "isampler2D"),
        "usampler2D" => ("utexture2D", "sampler", "usampler2D"),
        "sampler3D" => ("texture3D", "sampler", "sampler3D"),
        "isampler3D" => ("itexture3D", "sampler", "isampler3D"),
        "usampler3D" => ("utexture3D", "sampler", "usampler3D"),
        "samplerCube" => ("textureCube", "sampler", "samplerCube"),
        "sampler2DArray" => ("texture2DArray", "sampler", "sampler2DArray"),
        "isampler2DArray" => ("itexture2DArray", "sampler", "isampler2DArray"),
        "usampler2DArray" => ("utexture2DArray", "sampler", "usampler2DArray"),
        "sampler2DShadow" => ("texture2D", "samplerShadow", "sampler2DShadow"),
        "samplerCubeShadow" => ("textureCube", "samplerShadow", "samplerCubeShadow"),
        "sampler2DArrayShadow" => ("texture2DArray", "samplerShadow", "sampler2DArrayShadow"),
        _ => return None,
    })
}

const QUALIFIERS: [&str; 9] = [
    "highp",
    "mediump",
    "lowp",
    "flat",
    "smooth",
    "noperspective",
    "centroid",
    "invariant",
    "precise",
];

/// Rewrites a GLSL ES 3.00 fragment shader as GLSL 450 that naga reads.
pub(crate) fn convert(source: &str) -> Result<Converted, String> {
    let toks = tokens(source);
    let text = |t: &Token| &source[t.range.0..t.range.1];
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut layout = GlslLayout::default();
    let mut next_binding = 0u32;
    let mut used_locations: Vec<u32> = Vec::new();
    let mut pending_inputs: Vec<(usize, String)> = Vec::new();
    let mut outputs: Vec<usize> = Vec::new();
    // Sampler uniforms by name, with the constructor their uses are wrapped in.
    let mut samplers: HashMap<String, &'static str> = HashMap::new();
    // Token ranges of the declarations rewritten whole, whose names aren't uses.
    let mut replaced: Vec<(usize, usize)> = Vec::new();

    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if t.kind == Kind::Directive {
            let line = text(&t);
            if line
                .trim_start_matches('#')
                .trim_start()
                .starts_with("version")
            {
                edits.push((t.range.0..t.range.1, "#version 450".to_string()));
            }
            i += 1;
            continue;
        }
        // A declaration at file scope runs to its `;`, or a function to its closing brace.
        let start = i;
        let mut end = i;
        let mut inner = 0i32;
        let mut function = false;
        while end < toks.len() {
            let s = text(&toks[end]);
            if toks[end].kind == Kind::Directive && inner == 0 {
                break;
            }
            match s {
                "{" => {
                    inner += 1;
                    // A brace after `)` starts a function body.
                    if inner == 1 && end > start && text(&toks[end - 1]) == ")" {
                        function = true;
                    }
                }
                "}" => {
                    inner -= 1;
                    if inner == 0 && function {
                        end += 1;
                        break;
                    }
                }
                ";" if inner == 0 => {
                    end += 1;
                    break;
                }
                _ => {}
            }
            end += 1;
        }
        if !function {
            declaration(
                source,
                &toks[start..end],
                &mut edits,
                &mut layout,
                &mut next_binding,
                &mut used_locations,
                &mut pending_inputs,
                &mut outputs,
                &mut samplers,
                &mut replaced,
            )?;
        }
        i = end.max(start + 1);
    }

    // Inputs and outputs without a location get the next free one, in source order.
    let mut free = (0u32..).filter(|l| !used_locations.contains(l));
    for (at, name) in pending_inputs {
        let location = free.next().unwrap_or(0);
        edits.push((at..at, format!("layout(location = {location}) ")));
        layout.inputs.push(GlslInput { name, location });
    }
    for (k, at) in outputs.into_iter().enumerate() {
        edits.push((at..at, format!("layout(location = {k}) ")));
    }

    // Uses of a sampler uniform become a combined sampler built from its two halves.
    for t in &toks {
        if t.kind != Kind::Ident
            || replaced
                .iter()
                .any(|&(a, b)| t.range.0 >= a && t.range.1 <= b)
        {
            continue;
        }
        let name = text(t);
        let Some(constructor) = samplers.get(name) else {
            continue;
        };
        let after_dot = source[..t.range.0].trim_end().ends_with('.');
        if !after_dot {
            edits.push((
                t.range.0..t.range.1,
                format!("{constructor}({name}, {name}_sampler)"),
            ));
        }
    }

    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let mut text_out = String::with_capacity(source.len() + 256);
    let mut map = Vec::new();
    let mut at = 0;
    for (range, replacement) in edits {
        if range.start < at {
            return Err("overlapping GLSL rewrites".to_string());
        }
        text_out.push_str(&source[at..range.start]);
        let converted_start = text_out.len();
        text_out.push_str(&replacement);
        map.push((range.clone(), converted_start..text_out.len()));
        at = range.end;
    }
    text_out.push_str(&source[at..]);
    Ok(Converted {
        text: text_out,
        layout,
        edits: map,
    })
}

/// Rewrites one declaration at file scope, when it is an input, output or uniform.
#[allow(clippy::too_many_arguments)]
fn declaration(
    source: &str,
    toks: &[Token],
    edits: &mut Vec<(Range<usize>, String)>,
    layout: &mut GlslLayout,
    next_binding: &mut u32,
    used_locations: &mut Vec<u32>,
    pending_inputs: &mut Vec<(usize, String)>,
    outputs: &mut Vec<usize>,
    samplers: &mut HashMap<String, &'static str>,
    replaced: &mut Vec<(usize, usize)>,
) -> Result<(), String> {
    let text = |t: &Token| &source[t.range.0..t.range.1];
    let Some(first) = toks.first() else {
        return Ok(());
    };
    // Skip any layout(...) qualifier, noting a location it gives.
    let mut k = 0;
    let mut location: Option<u32> = None;
    let mut layout_range: Option<(usize, usize)> = None;
    while k < toks.len() && text(&toks[k]) == "layout" {
        let open = k + 1;
        let mut close = open;
        while close < toks.len() && text(&toks[close]) != ")" {
            close += 1;
        }
        for w in open..close {
            if text(&toks[w]) == "location" {
                location = toks.get(w + 2).and_then(|v| text(v).parse().ok());
            }
        }
        layout_range = Some((
            toks[k].range.0,
            toks.get(close).map_or(toks[k].range.1, |c| c.range.1),
        ));
        k = close + 1;
    }
    while k < toks.len() && QUALIFIERS.contains(&text(&toks[k])) {
        k += 1;
    }
    let Some(storage) = toks.get(k) else {
        return Ok(());
    };
    let insert_at = toks.first().map_or(0, |t| t.range.0);
    let whole = toks[0].range.0..toks.last().map_or(toks[0].range.1, |t| t.range.1);
    match text(storage) {
        // Samplers become textures and samplers, so a precision for the sampler type goes.
        "precision" if toks.iter().any(|t| sampler_parts(text(t)).is_some()) => {
            edits.push((whole, String::new()));
        }
        "in" => {
            let name = toks
                .iter()
                .rev()
                .find(|t| t.kind == Kind::Ident)
                .map(|t| text(t).to_string())
                .unwrap_or_default();
            match location {
                Some(l) => {
                    used_locations.push(l);
                    layout.inputs.push(GlslInput { name, location: l });
                }
                None => pending_inputs.push((insert_at, name)),
            }
        }
        "out" if location.is_none() => outputs.push(insert_at),
        "uniform" => {
            let after = &toks[k + 1..];
            let is_block = after.len() >= 2 && text(&after[1]) == "{";
            if is_block {
                let name = text(&after[0]).to_string();
                let binding = take(next_binding, 1);
                // A block keeps its std140 layout and gets a binding.
                let range = match layout_range {
                    Some((a, b)) => a..b,
                    None => first.range.0..first.range.0,
                };
                let lead = if layout_range.is_some() { "" } else { " " };
                edits.push((
                    range,
                    format!("layout(std140, set = 0, binding = {binding}){lead}"),
                ));
                layout.uniforms.push(GlslUniform {
                    name,
                    kind: GlslUniformKind::Block,
                    binding,
                });
                return Ok(());
            }
            // The type is the last identifier before the first declared name.
            let mut j = 0;
            while j < after.len() && QUALIFIERS.contains(&text(&after[j])) {
                j += 1;
            }
            let Some(ty) = after.get(j) else {
                return Ok(());
            };
            let ty_text = text(ty);
            let precision: String = after[..j].iter().map(|t| format!("{} ", text(t))).collect();
            // Declarators: name, optional [size], separated by commas, up to the `;`.
            let mut names: Vec<(String, String)> = Vec::new();
            let mut d = j + 1;
            while d < after.len() {
                let name = text(&after[d]).to_string();
                let mut array = String::new();
                d += 1;
                if after.get(d).is_some_and(|t| text(t) == "[") {
                    let open = d;
                    while d < after.len() && text(&after[d]) != "]" {
                        d += 1;
                    }
                    array = source[after[open].range.0
                        ..after.get(d).map_or(after[open].range.1, |c| c.range.1)]
                        .to_string();
                    d += 1;
                }
                names.push((name, array));
                if after.get(d).is_some_and(|t| text(t) == ",") {
                    d += 1;
                    continue;
                }
                break;
            }
            replaced.push((whole.start, whole.end));
            let mut out = Vec::new();
            for (name, array) in names {
                if let Some((texture, sampler, constructor)) = sampler_parts(ty_text) {
                    let t = take(next_binding, 2);
                    out.push(format!(
                        "layout(set = 0, binding = {t}) uniform {texture} {name}{array}; layout(set = 0, binding = {}) uniform {sampler} {name}_sampler;",
                        t + 1
                    ));
                    samplers.insert(name.clone(), constructor);
                    layout.uniforms.push(GlslUniform {
                        name: name.clone(),
                        kind: GlslUniformKind::Texture,
                        binding: t,
                    });
                    layout.uniforms.push(GlslUniform {
                        name,
                        kind: GlslUniformKind::Sampler,
                        binding: t + 1,
                    });
                } else {
                    let b = take(next_binding, 1);
                    out.push(format!(
                        "layout(std140, set = 0, binding = {b}) uniform Texelscope_{name} {{ {precision}{ty_text} {name}{array}; }};"
                    ));
                    layout.uniforms.push(GlslUniform {
                        name,
                        kind: GlslUniformKind::Value,
                        binding: b,
                    });
                }
            }
            edits.push((whole, out.join(" ")));
        }
        _ => {}
    }
    Ok(())
}

fn take(next: &mut u32, count: u32) -> u32 {
    let at = *next;
    *next += count;
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    const ES: &str = "#version 300 es
precision highp float;
precision highp sampler2D;
// A comment with uniform float fake;
uniform float u_scale;
uniform highp vec2 u_pair, u_other;
uniform sampler2D u_tex;
layout(std140) uniform Params { float k; } params;
in vec2 v_uv;
flat in int v_id;
out vec4 color;
float f(float x) { return x * u_scale; }
void main() {
    color = texture(u_tex, v_uv) * f(params.k) + vec4(u_pair, u_other) + float(v_id);
}
";

    #[test]
    fn rewrites_es_for_naga_and_keeps_lines() {
        let c = convert(ES).unwrap();
        assert_eq!(c.text.lines().count(), ES.lines().count(), "{}", c.text);
        assert!(c.text.starts_with("#version 450\n"), "{}", c.text);
        assert!(
            c.text
                .contains("uniform Texelscope_u_scale { float u_scale; };"),
            "{}",
            c.text
        );
        assert!(
            c.text
                .contains("uniform Texelscope_u_pair { highp vec2 u_pair; };"),
            "{}",
            c.text
        );
        assert!(c.text.contains("uniform texture2D u_tex; layout(set = 0, binding = 4) uniform sampler u_tex_sampler;"), "{}", c.text);
        assert!(
            c.text.contains(
                "layout(std140, set = 0, binding = 5) uniform Params { float k; } params;"
            ),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("layout(location = 0) in vec2 v_uv;"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("layout(location = 1) flat in int v_id;"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("layout(location = 0) out vec4 color;"),
            "{}",
            c.text
        );
        assert!(
            c.text
                .contains("texture(sampler2D(u_tex, u_tex_sampler), v_uv)"),
            "{}",
            c.text
        );
        assert!(
            c.text.contains("// A comment with uniform float fake;"),
            "{}",
            c.text
        );
        assert_eq!(
            c.layout.inputs,
            [
                GlslInput {
                    name: "v_uv".into(),
                    location: 0
                },
                GlslInput {
                    name: "v_id".into(),
                    location: 1
                },
            ]
        );
        let kinds: Vec<(&str, GlslUniformKind, u32)> = c
            .layout
            .uniforms
            .iter()
            .map(|u| (u.name.as_str(), u.kind, u.binding))
            .collect();
        assert_eq!(
            kinds,
            [
                ("u_scale", GlslUniformKind::Value, 0),
                ("u_pair", GlslUniformKind::Value, 1),
                ("u_other", GlslUniformKind::Value, 2),
                ("u_tex", GlslUniformKind::Texture, 3),
                ("u_tex", GlslUniformKind::Sampler, 4),
                ("Params", GlslUniformKind::Block, 5),
            ]
        );
    }

    #[test]
    fn maps_offsets_back_to_the_original() {
        let c = convert(ES).unwrap();
        let find = |s: &str, needle: &str| s.find(needle).unwrap();
        for needle in ["f(params.k)", "float f(float x)", "v_uv) *"] {
            assert_eq!(
                c.original(find(&c.text, needle), false),
                find(ES, needle),
                "{needle}"
            );
        }
        // Inside the rewritten sampler use, an offset maps to the name it replaced.
        let use_at = find(&c.text, "sampler2D(u_tex, u_tex_sampler)");
        assert_eq!(c.original(use_at + 5, false), find(ES, "u_tex, v_uv"));
    }
}
