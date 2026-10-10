//! How much each input moves a pixel's color, by central differences: nudge one input up
//! and down, run the pixel again and compare.

use std::collections::BTreeSet;

use texelscope_capture::Resource;
use texelscope_interp::{Interpreter, Origin, Perturbation, Trace};
use texelscope_ir::{Override, Value};

use crate::ExplainError;
use crate::model::Sensitivity;

/// Inputs measured per pixel, so a pixel that reads thousands of values stays quick.
const MAX_INPUTS: usize = 64;
/// Sensitivities reported, most first.
const MAX_REPORTED: usize = 12;

struct Candidate {
    input: String,
    value: f64,
    perturbation: Perturbation,
}

pub(crate) fn measure(
    interp: &Interpreter,
    trace: &Trace,
    overrides: &[Override],
) -> Result<Vec<Sensitivity>, ExplainError> {
    let mut out = Vec::new();
    for candidate in candidates(interp, trace) {
        let step = (candidate.value.abs() * 1e-3).max(1e-3) as f32;
        let shade = |delta: f32| -> Result<Option<[f32; 4]>, ExplainError> {
            let nudged = with_delta(&candidate.perturbation, delta);
            Ok(interp
                .shade_pixel(trace.pixel, overrides, Some(&nudged))?
                .color())
        };
        let (Some(up), Some(down)) = (shade(step)?, shade(-step)?) else {
            continue;
        };
        let d_color: [f64; 4] = std::array::from_fn(|i| {
            (f64::from(up[i]) - f64::from(down[i])) / (2.0 * f64::from(step))
        });
        if d_color.iter().all(|d| *d == 0.0 || !d.is_finite()) {
            continue;
        }
        out.push(Sensitivity {
            input: candidate.input,
            value: candidate.value,
            d_color,
            perturbation: Some(candidate.perturbation),
        });
    }
    let weight = |s: &Sensitivity| {
        s.d_color
            .iter()
            .filter(|d| d.is_finite())
            .map(|d| d.abs())
            .sum::<f64>()
    };
    out.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
    out.truncate(MAX_REPORTED);
    Ok(out)
}

/// Every float the pixel read from outside the shader, one perturbation each.
fn candidates(interp: &Interpreter, trace: &Trace) -> Vec<Candidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for step in &trace.steps {
        let Some(origin) = &step.origin else {
            continue;
        };
        match origin {
            Origin::Buffer {
                group,
                binding,
                path,
                indices,
            } => {
                for (suffix, label, value) in float_lanes(&step.value) {
                    let mut indices = indices.clone();
                    indices.extend(suffix);
                    let input = format!("{path}{label}");
                    if seen.insert(input.clone()) {
                        out.push(Candidate {
                            input,
                            value,
                            perturbation: Perturbation::Buffer {
                                group: *group,
                                binding: *binding,
                                indices,
                                delta: 0.0,
                            },
                        });
                    }
                }
            }
            Origin::Texels {
                group,
                binding,
                name,
                level,
                texels,
            } => {
                let Some(texture) = interp.data().texture(*group, *binding) else {
                    continue;
                };
                let components = texture_components(interp, *group, *binding);
                for &texel in texels {
                    for component in 0..components {
                        let input = if components == 1 {
                            format!("{name} texel ({}, {})", texel[0], texel[1])
                        } else {
                            format!(
                                "{name} texel ({}, {}).{}",
                                texel[0],
                                texel[1],
                                ['r', 'g', 'b', 'a'][component]
                            )
                        };
                        if !seen.insert(input.clone()) {
                            continue;
                        }
                        let at = texel.map(|c| c.max(0) as u32);
                        let value = f64::from(texture.texel(*level as usize, at)[component]);
                        out.push(Candidate {
                            input,
                            value,
                            perturbation: Perturbation::Texel {
                                group: *group,
                                binding: *binding,
                                level: *level,
                                texel,
                                component: component as u32,
                                delta: 0.0,
                            },
                        });
                    }
                }
            }
            Origin::Input { .. } => {}
        }
        if out.len() >= MAX_INPUTS {
            out.truncate(MAX_INPUTS);
            break;
        }
    }
    out
}

/// The f32 lanes of a loaded value, with the index and label that reach each one.
fn float_lanes(value: &Value) -> Vec<(Vec<u32>, String, f64)> {
    match value {
        Value::F32(v) => vec![(Vec::new(), String::new(), f64::from(*v))],
        Value::Vector(items) => items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                Value::F32(v) => Some((
                    vec![i as u32],
                    format!(".{}", ['x', 'y', 'z', 'w'][i.min(3)]),
                    f64::from(*v),
                )),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn texture_components(interp: &Interpreter, group: u32, binding: u32) -> usize {
    interp
        .capture()
        .bindings
        .iter()
        .find(|b| b.group == group && b.binding == binding)
        .and_then(|b| match &b.resource {
            Resource::Texture(t) => Some(t.format.components() as usize),
            _ => None,
        })
        .unwrap_or(1)
}

fn with_delta(perturbation: &Perturbation, delta: f32) -> Perturbation {
    let mut p = perturbation.clone();
    match &mut p {
        Perturbation::Buffer { delta: d, .. }
        | Perturbation::Texel { delta: d, .. }
        | Perturbation::Input { delta: d, .. } => *d = delta,
    }
    p
}
