import { describe, expect, it } from 'vitest';
import type { BranchLayer, Layer, RegionData } from '../src/types.js';
import { arrange } from '../src/views/region-cards.js';

const layer = (name: string, order: number, min: number, max: number, extra: Partial<Layer> = {}): Layer => ({
  name,
  function: 'fs',
  expr: null,
  span: null,
  components: 1,
  min: [min, 0, 0, 0],
  max: [max, 0, 0, 0],
  evaluations: 1,
  reached: 4,
  order,
  setting: false,
  ...extra,
});

const branch = (order: number, taken: number, notTaken: number, extra: Partial<BranchLayer> = {}): BranchLayer => ({
  condition: { function: { kind: 'entryPoint', index: 0 }, expr: order },
  span: null,
  taken,
  notTaken,
  mixed: 0,
  order,
  setting: false,
  ...extra,
});

const meta = (layers: Layer[], branches: BranchLayer[]): RegionData['meta'] => ({
  rect: { x: 0, y: 0, width: 2, height: 2 },
  layers,
  branches,
});

describe('arrange', () => {
  it('puts values and decisions in one compute order', () => {
    const arranged = arrange(meta([layer('x', 1, 0, 3), layer('y', 5, 1, 2)], [branch(3, 2, 2)]));
    expect(arranged.varying).toEqual([
      { kind: 'layer', index: 0 },
      { kind: 'branch', index: 0 },
      { kind: 'layer', index: 1 },
    ]);
  });

  it('moves what every pixel shares out of the strip, with settings last', () => {
    const arranged = arrange(
      meta(
        [layer('scale', 1, 2, 2, { setting: true }), layer('t', 4, 0, 0), layer('x', 2, 0, 3)],
        [branch(3, 4, 0, { setting: true }), branch(5, 0, 4)],
      ),
    );
    expect(arranged.varying).toEqual([{ kind: 'layer', index: 2 }]);
    expect(arranged.same).toEqual([
      { kind: 'layer', index: 1 },
      { kind: 'branch', index: 1 },
    ]);
    expect(arranged.settings).toEqual([
      { kind: 'layer', index: 0 },
      { kind: 'branch', index: 0 },
    ]);
  });

  it('keeps a card in the strip when some pixels never reach it', () => {
    const arranged = arrange(meta([layer('vj', 1, 7, 7, { reached: 3 })], [branch(2, 3, 0), branch(4, 0, 0, { mixed: 4 })]));
    expect(arranged.varying).toEqual([
      { kind: 'layer', index: 0 },
      { kind: 'branch', index: 0 },
      { kind: 'branch', index: 1 },
    ]);
    expect(arranged.same).toEqual([]);
  });
});
