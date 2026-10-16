import { describe, expect, it } from 'vitest';
import type { Capture } from '@texelscope/sdk';
import { decodeTexel, quantize, shownPixel, toBytes, vertexStage } from '../src/gpu/replay.js';
import type { Description } from '../src/types.js';

const capture = {
  version: 0,
  shader: { language: 'wgsl', source: { inline: '' }, entryPoint: 'fs' },
  target: { width: 4, height: 4, format: 'rgba8unorm' },
  inputs: { kind: 'fullscreen', locations: [{ location: 0, corners: [[0, 0], [1, 0], [0, 1], [1, 1]] }] },
} as Capture;

const description = {
  stageInputs: [
    { location: 0, ty: 'vec2<f32>', interpolation: 'perspective', sampling: 'center' },
    { location: 2, ty: 'u32', interpolation: 'flat', sampling: null },
  ],
} as unknown as Description;

describe('the generated vertex stage', () => {
  const wgsl = vertexStage(capture, description);

  it('declares every input the fragment stage reads, with its interpolation', () => {
    expect(wgsl).toContain('@location(0) @interpolate(perspective, center) v0: vec2<f32>,');
    expect(wgsl).toContain('@location(2) @interpolate(flat) v2: u32,');
  });

  it('extends the corner values across the covering triangle', () => {
    expect(wgsl).toContain('out.v0 = vec2<f32>((vec4<f32>(0.000000000');
    // A location the capture does not give reads zero, as in the interpreter.
    expect(wgsl).toContain('out.v2 = u32();');
  });
});

describe('texels read back from the GPU', () => {
  it('swap blue and red for bgra8unorm', () => {
    expect(decodeTexel('bgra8unorm', new Uint8Array([255, 0, 51, 255]))).toEqual([0.2, 0, 1, 1]);
  });

  it('read floats as stored', () => {
    const bytes = new Uint8Array(new Float32Array([1.5, -2]).buffer);
    expect(decodeTexel('rg32float', bytes)).toEqual([1.5, -2, 0, 1]);
  });

  it('compare with CPU colors rounded the way the target stores them', () => {
    expect(quantize('rgba8unorm', [0.5, 1.2, -1, 0.1])).toEqual([128 / 255, 1, 0, 26 / 255]);
    expect(quantize('rgba32float', [0.1])).toEqual([0.1]);
  });
});

describe('images the app drew again', () => {
  it('show clear where the draw never reached and clamp what it wrote', () => {
    const data = new Float32Array([NaN, NaN, NaN, NaN, 0.5, 1.5, -1, 1]);
    expect(Array.from(toBytes(data))).toEqual([0, 0, 0, 0, 128, 255, 0, 255]);
  });

  it('read back as the target stores them, and not at all where nothing drew', () => {
    const shown = { width: 2, data: new Float32Array([NaN, NaN, NaN, NaN, 0.5, 1.5, -1, 1]) };
    expect(shownPixel(shown, 0, 0, 'rgba8unorm')).toEqual([]);
    expect(shownPixel(shown, 1, 0, 'rgba8unorm')).toEqual([128 / 255, 1, 0, 1]);
    expect(shownPixel({ width: 1, data: new Uint8Array([255, 0, 51, 255]) }, 0, 0, 'rgba8unorm')).toEqual([1, 0, 0.2, 1]);
    // A canvas that shrank in the Live view no longer has the picked pixel.
    expect(shownPixel(shown, 2, 0, 'rgba8unorm')).toEqual([]);
    expect(shownPixel(shown, 0, 1, 'rgba8unorm')).toEqual([]);
  });
});
