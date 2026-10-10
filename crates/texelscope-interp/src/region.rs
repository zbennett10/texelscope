//! Named values and decisions over a rectangle of pixels, as images.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use texelscope_capture::Rect;
use texelscope_ir::{ExprRef, Override, SourceSpan};

use crate::lane::{Lane, Overrides};
use crate::record::{LayerKey, LayerSink, Record};
use crate::{InterpError, Interpreter};

/// The largest region one call inspects.
pub const MAX_REGION_PIXELS: u32 = 512 * 512;

/// Every named value and decision over a rectangle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub rect: Rect,
    /// Named values in the order a pixel first computes them.
    pub layers: Vec<Layer>,
    pub branches: Vec<BranchLayer>,
    /// RGBA for each pixel, NaN where the invocation discarded.
    #[serde(skip)]
    pub colors: Vec<f32>,
}

/// One named value over a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub name: String,
    pub function: String,
    /// The `let` this layer shows, or `None` for a `var`.
    pub expr: Option<ExprRef>,
    pub span: Option<SourceSpan>,
    pub components: u32,
    /// Smallest and largest value of each component over the pixels that computed it.
    pub min: [f32; 4],
    pub max: [f32; 4],
    /// The most times any one pixel computed it, such as once per loop iteration.
    pub evaluations: u32,
    /// Pixels that computed it at least once.
    pub reached: u32,
    /// Four floats per pixel, rows from the top, NaN where a pixel did not compute it.
    #[serde(skip)]
    pub data: Vec<f32>,
}

/// One branch condition over a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchLayer {
    pub condition: ExprRef,
    pub span: Option<SourceSpan>,
    /// Per pixel: 0 not reached, 1 false, 2 true, 3 both on different iterations.
    #[serde(skip)]
    pub data: Vec<u8>,
    pub taken: u32,
    pub not_taken: u32,
}

struct Accumulator {
    layer: Layer,
    order: u64,
}

pub(crate) fn inspect(
    interp: &Interpreter,
    rect: Rect,
    overrides: &[Override],
    scope: Option<Rect>,
    layered: bool,
) -> Result<Region, InterpError> {
    let target = interp.capture().target;
    let rect = clip(rect, target.width, target.height);
    if layered && rect.width * rect.height > MAX_REGION_PIXELS {
        return Err(InterpError::Unsupported(format!(
            "regions are limited to {MAX_REGION_PIXELS} pixels, and this one has {}",
            rect.width * rect.height
        )));
    }
    let patched = interp.overrides(overrides)?;
    let plain = Overrides::default();
    let applies = |x: u32, y: u32| scope.is_none_or(|s| s.contains(x, y));
    let count = (rect.width * rect.height) as usize;
    let mut colors = vec![f32::NAN; count * 4];
    let mut layers: HashMap<LayerKey, Accumulator> = HashMap::new();
    let mut branches: HashMap<ExprRef, (BranchLayer, u64)> = HashMap::new();
    let names = interp.function_names();

    let step = if interp.uses_quads() { 2 } else { 1 };
    let (x0, y0) = if step == 2 {
        (rect.x & !1, rect.y & !1)
    } else {
        (rect.x, rect.y)
    };
    let (x_end, y_end) = (rect.x + rect.width, rect.y + rect.height);
    for qy in (y0..y_end).step_by(step) {
        for qx in (x0..x_end).step_by(step) {
            let mut lanes: Vec<Lane<'_>> = Vec::with_capacity(step * step);
            for dy in 0..step as u32 {
                for dx in 0..step as u32 {
                    let (x, y) = (qx + dx, qy + dy);
                    let inside = rect.contains(x, y);
                    let record = if inside && layered {
                        Record::Layers(Box::default())
                    } else {
                        Record::Off
                    };
                    let overrides = if applies(x, y) { &patched } else { &plain };
                    lanes.push(Lane::new(interp, [x, y], overrides, None, record));
                }
            }
            interp.run_quad(&mut lanes)?;
            for lane in lanes {
                let [x, y] = lane.pixel;
                if !rect.contains(x, y) {
                    continue;
                }
                let index = ((y - rect.y) * rect.width + (x - rect.x)) as usize;
                if !lane.discarded
                    && let Some((_, _, value)) = lane.outputs.iter().find(|(l, ..)| *l == Some(0))
                    && let Some(lanes) = value.lanes()
                {
                    let mut rgba = [0.0, 0.0, 0.0, 1.0];
                    for (o, s) in rgba.iter_mut().zip(lanes) {
                        *o = s.as_f32();
                    }
                    colors[index * 4..index * 4 + 4].copy_from_slice(&rgba);
                }
                let Record::Layers(sink) = lane.record else {
                    continue;
                };
                merge(*sink, index, count, &names, &mut layers, &mut branches);
            }
        }
    }

    let mut layers: Vec<Accumulator> = layers.into_values().collect();
    layers.sort_by_key(|a| a.order);
    let mut branches: Vec<(BranchLayer, u64)> = branches.into_values().collect();
    branches.sort_by_key(|(_, order)| *order);
    Ok(Region {
        rect,
        layers: layers.into_iter().map(|a| a.layer).collect(),
        branches: branches.into_iter().map(|(b, _)| b).collect(),
        colors,
    })
}

fn merge(
    sink: LayerSink,
    index: usize,
    count: usize,
    names: &HashMap<texelscope_ir::FunctionRef, String>,
    layers: &mut HashMap<LayerKey, Accumulator>,
    branches: &mut HashMap<ExprRef, (BranchLayer, u64)>,
) {
    for (key, sample) in sink.samples {
        let expr = match &key {
            LayerKey::Expr(e) | LayerKey::Member(e, _) => Some(*e),
            LayerKey::Var(..) => None,
        };
        let order = u64::from(sample.order);
        let acc = layers.entry(key).or_insert_with(|| Accumulator {
            layer: Layer {
                name: sample.name.clone(),
                function: names.get(&sample.function).cloned().unwrap_or_default(),
                expr,
                span: sample.span,
                components: u32::from(sample.components),
                min: [f32::INFINITY; 4],
                max: [f32::NEG_INFINITY; 4],
                evaluations: 0,
                reached: 0,
                data: vec![f32::NAN; count * 4],
            },
            order,
        });
        acc.order = acc.order.min(order);
        let layer = &mut acc.layer;
        layer.evaluations = layer.evaluations.max(sample.count);
        layer.reached += 1;
        for c in 0..usize::from(sample.components) {
            let v = sample.lanes[c];
            layer.data[index * 4 + c] = v;
            if v.is_finite() {
                layer.min[c] = layer.min[c].min(v);
                layer.max[c] = layer.max[c].max(v);
            }
        }
    }
    for (condition, sample) in sink.branches {
        let order = u64::from(sample.order);
        let (layer, first) = branches.entry(condition).or_insert_with(|| {
            (
                BranchLayer {
                    condition,
                    span: sample.span,
                    data: vec![0; count],
                    taken: 0,
                    not_taken: 0,
                },
                order,
            )
        });
        *first = (*first).min(order);
        layer.data[index] = if sample.mixed {
            3
        } else if sample.taken {
            2
        } else {
            1
        };
        if sample.taken {
            layer.taken += 1;
        } else {
            layer.not_taken += 1;
        }
    }
}

fn clip(rect: Rect, width: u32, height: u32) -> Rect {
    let x = rect.x.min(width);
    let y = rect.y.min(height);
    Rect {
        x,
        y,
        width: rect.width.min(width - x),
        height: rect.height.min(height - y),
    }
}
