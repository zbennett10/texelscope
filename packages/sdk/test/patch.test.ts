import { describe, expect, it } from 'vitest';
import { patch, restoreAll } from '../src/webgpu/patch.js';

describe('restoreAll', () => {
  it('puts back every patched method', () => {
    const proto = { a: () => 'a', b: () => 'b', c: () => 'c', d: () => 'd' };
    for (const key of ['a', 'b', 'c', 'd'] as const) {
      patch(proto, key, () => () => 'patched');
    }
    restoreAll();
    expect([proto.a(), proto.b(), proto.c(), proto.d()]).toEqual(['a', 'b', 'c', 'd']);
  });
});
