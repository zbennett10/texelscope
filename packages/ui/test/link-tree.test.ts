import { describe, expect, it } from 'vitest';
import { callTree, type LinkPlace } from '../src/views/link-tree.js';

const at = (index: number, depth: number, call: number, scope: string | null): LinkPlace => ({ index, depth, call, scope });

describe('callTree', () => {
  it('nests values under the calls that computed them', () => {
    const tree = callTree([
      at(0, 0, 0, null),
      at(1, 1, 1, 'shade'),
      at(2, 2, 2, 'nearest'),
      at(3, 2, 2, 'nearest'),
      at(4, 1, 1, 'shade'),
      at(5, 0, 0, null),
    ]);
    expect(tree).toEqual([
      { kind: 'row', index: 0 },
      {
        kind: 'call',
        key: '/shade#0',
        name: 'shade',
        size: 4,
        children: [
          { kind: 'row', index: 1 },
          {
            kind: 'call',
            key: '/shade#0/nearest#0',
            name: 'nearest',
            size: 2,
            children: [
              { kind: 'row', index: 2 },
              { kind: 'row', index: 3 },
            ],
          },
          { kind: 'row', index: 4 },
        ],
      },
      { kind: 'row', index: 5 },
    ]);
  });

  it('keeps back to back calls apart and numbers them', () => {
    const tree = callTree([at(0, 1, 1, 'sample'), at(1, 1, 1, 'sample'), at(2, 1, 2, 'sample'), at(3, 1, 2, 'sample')]);
    expect(tree.map((n) => (n.kind === 'call' ? n.key : n.index))).toEqual(['/sample#0', '/sample#1']);
  });

  it('names a call by its own depth when a deeper call comes first', () => {
    const tree = callTree([at(0, 2, 2, 'nearest'), at(1, 1, 1, 'shade')]);
    expect(tree[0]).toMatchObject({ kind: 'call', name: 'shade', size: 2 });
  });

  it('leaves a call with a single value as a plain row', () => {
    expect(callTree([at(0, 0, 0, null), at(1, 1, 1, 'nearest'), at(2, 0, 0, null)])).toEqual([
      { kind: 'row', index: 0 },
      { kind: 'row', index: 1 },
      { kind: 'row', index: 2 },
    ]);
  });
});
