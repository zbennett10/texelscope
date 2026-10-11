import { describe, expect, it } from 'vitest';
import type { BranchLayer, Layer, RegionData } from '../src/types.js';
import { findEdge, twoColors } from '../src/views/region-edge.js';

const layer = (name: string): Layer => ({
  name,
  function: 'fs',
  expr: null,
  span: null,
  components: 1,
  min: [0, 0, 0, 0],
  max: [0, 0, 0, 0],
  evaluations: 1,
  reached: 4,
  order: 0,
  setting: false,
});

const branch: BranchLayer = {
  condition: { function: { kind: 'entryPoint', index: 0 }, expr: 1 },
  span: null,
  taken: 2,
  notTaken: 2,
  mixed: 0,
  order: 0,
  setting: false,
};

/** Four pixels in a row, black on the left and blue on the right. */
function region(values: number[][], decisions: number[][]): RegionData {
  const black = [0, 0, 0, 1];
  const blue = [0.2, 0.3, 0.9, 1];
  const layers = new Float32Array(values.length * 16).fill(Number.NaN);
  values.forEach((vs, l) => vs.forEach((v, p) => (layers[l * 16 + p * 4] = v)));
  return {
    meta: { rect: { x: 0, y: 0, width: 4, height: 1 }, layers: values.map((_, i) => layer(`v${i}`)), branches: decisions.map(() => branch) },
    layers,
    branches: Uint8Array.from(decisions.flat()),
    colors: Float32Array.from([...black, ...black, ...blue, ...blue]),
  };
}

describe('findEdge', () => {
  it('finds the first card that splits the pixels the way the colors do', () => {
    const data = region([[5, 1, 5, 1], [0, 1, 2, 3]], [[1, 1, 2, 2]]);
    const edge = findEdge(data, [
      { kind: 'layer', index: 0 },
      { kind: 'layer', index: 1 },
      { kind: 'branch', index: 0 },
    ]);
    expect(edge?.start).toEqual({ kind: 'layer', index: 1 });
    expect(edge?.full).toEqual({ kind: 'layer', index: 1 });
  });

  it('counts pixels that never computed a value as their own group', () => {
    const data = region([[7, 7, Number.NaN, Number.NaN]], []);
    expect(findEdge(data, [{ kind: 'layer', index: 0 }])?.start).toEqual({ kind: 'layer', index: 0 });
  });

  it('tells where an edge starts to show from where it is complete', () => {
    // Pixel 1 is dark too, but only the last card puts it with the dark side.
    const data = region([[0, 2, 2, 3], [0, 0, 2, 3]], []);
    const edge = findEdge(data, [
      { kind: 'layer', index: 0 },
      { kind: 'layer', index: 1 },
    ], 0.4);
    expect(edge?.start).toEqual({ kind: 'layer', index: 0 });
    expect(edge?.full).toEqual({ kind: 'layer', index: 1 });
  });

  it('finds nothing in a region of one color', () => {
    const one = Float32Array.from(Array(4).fill([0.5, 0.5, 0.5, 1]).flat());
    expect(twoColors(one, [0, 1, 2, 3])).toBeNull();
  });
});
