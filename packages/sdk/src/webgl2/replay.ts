// Draws a kept draw again with another fragment shader, a what-if or a probe, at the full
// size of its target, and reads every pixel back as floats.

import { drawKept, type KeptDraw, programFor } from './keep.js';
import { flipRows } from './read.js';

/** One draw as a fragment shader drew it again. */
export interface Replayed {
  width: number;
  height: number;
  /** Four floats per pixel, rows from the top, NaN where the draw didn't reach a pixel. */
  data: Float32Array;
}

/** Draws the kept draw with `source` as its fragment shader, or says why it can't. */
export function replayDraw(kept: KeptDraw, source: string): Replayed | string {
  const gl = kept.gl;
  if (gl.isContextLost()) return "the app's WebGL context was lost";
  if (!gl.getExtension('EXT_color_buffer_float')) return "this GPU can't render to float targets, so the draw can't be replayed";
  const program = programFor(kept, source);
  if (typeof program === 'string') return program;
  const [data] = drawKept(kept, program, { x: 0, glY: 0, width: kept.width, height: kept.height }, 1);
  return { width: kept.width, height: kept.height, data: flipRows(data!, kept.width, kept.height, 4) };
}
