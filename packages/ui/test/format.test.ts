import { describe, expect, it } from 'vitest';
import { fixed, formatValue, numbers, parseLike, lineAt, lineStarts } from '../src/format.js';

describe('values', () => {
  it('spell like WGSL with short floats', () => {
    expect(formatValue({ vector: [{ f32: 0.123456 }, { f32: 2 }, { f32: 'NaN' }] })).toBe('vec3(0.1235, 2, NaN)');
    expect(formatValue({ u32: 3 })).toBe('3u');
    expect(formatValue({ struct: [['ri', { i32: -1 }]] })).toBe('{ ri: -1i }');
    expect(fixed(-0.00001)).toBe('0');
  });

  it('parse edits in the shape of the value they replace', () => {
    expect(parseLike({ f32: 1 }, '0.25')).toEqual({ f32: 0.25 });
    expect(parseLike({ vector: [{ f32: 0 }, { f32: 0 }] }, 'vec2(1, 2.5)')).toEqual({ vector: [{ f32: 1 }, { f32: 2.5 }] });
    expect(parseLike({ u32: 1 }, '-4')).toEqual({ u32: 0 });
    expect(parseLike({ vector: [{ f32: 0 }, { f32: 0 }] }, '1')).toBeUndefined();
    expect(parseLike({ f32: 0 }, 'abc')).toBeUndefined();
  });

  it('flatten scalars and vectors to numbers', () => {
    expect(numbers({ vector: [{ bool: true }, { i32: 4 }] })).toEqual([1, 4]);
  });
});

describe('lineAt', () => {
  it('finds the line of an offset, counted from 1', () => {
    const starts = lineStarts('ab\ncd\n\nef');
    expect(starts).toEqual([0, 3, 6, 7]);
    expect([0, 2, 3, 5, 6, 7, 8].map((o) => lineAt(starts, o))).toEqual([1, 1, 2, 2, 3, 4, 4]);
  });
});
