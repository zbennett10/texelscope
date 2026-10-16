// Reads the inputs a draw gave each pixel of a window, by drawing it again with a fragment
// shader that writes them out instead of a color.

import type { FragmentInputs, Rect } from '../capture.js';
import { toBase64 } from '../encode.js';
import { drawKept, type KeptDraw, programFor } from './keep.js';
import { flipRows } from './read.js';

/** One fragment input a GLSL ES shader declares. */
interface Varying {
  /** The declaration, without any location, ready to repeat in the reader. */
  declaration: string;
  name: string;
  components: number;
  /** How to widen it to the vec4 the reader writes. */
  packed: string;
}

const SHAPES: Record<string, [number, boolean]> = {
  float: [1, false],
  vec2: [2, false],
  vec3: [3, false],
  vec4: [4, false],
  int: [1, true],
  ivec2: [2, true],
  ivec3: [3, true],
  ivec4: [4, true],
  uint: [1, true],
  uvec2: [2, true],
  uvec3: [3, true],
  uvec4: [4, true],
};

/** The inputs a GLSL ES fragment shader declares at file scope. */
export function declaredInputs(source: string): Varying[] {
  const code = source.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/\/\/[^\n]*/g, ' ');
  // The `;` or `}` before a declaration is looked behind for, so the one ending it starts the next.
  const pattern = /(?<=^|[;}])\s*((?:layout\s*\([^)]*\)\s*)?(?:(?:flat|smooth|noperspective|centroid|highp|mediump|lowp)\s+)*)in\s+((?:(?:highp|mediump|lowp)\s+)?)(\w+)\s+(\w+)\s*;/g;
  const out: Varying[] = [];
  for (const m of code.matchAll(pattern)) {
    const [, qualifiers = '', precision = '', type = '', name = ''] = m;
    const shape = SHAPES[type];
    if (!shape) continue;
    const [components, integer] = shape;
    const kept = qualifiers.replace(/layout\s*\([^)]*\)/, '').trim();
    const widened = integer ? (components === 1 ? `float(${name})` : `vec${components}(${name})`) : name;
    const pad = Array(4 - components).fill('0.0');
    out.push({
      declaration: `${kept ? `${kept} ` : ''}in ${precision}${type} ${name};`,
      name,
      components,
      packed: components === 4 ? `vec4(${widened})` : `vec4(${[widened, ...pad].join(', ')})`,
    });
  }
  return out;
}

/** The uniform the reader adds to the fragment position to place it in the draw's target. */
const OFFSET = 'texelscope_offset';

/** Reads the fragment position and every declared input over `rect`, a window of the target. */
export function windowInputs(kept: KeptDraw, rect: Rect): FragmentInputs | string {
  const gl = kept.gl;
  if (gl.isContextLost()) return "the app's WebGL context was lost";
  if (!gl.getExtension('EXT_color_buffer_float')) return 'this GPU cannot render to float targets, so the inputs cannot be read';
  const varyings = declaredInputs(kept.source);
  const glY = kept.height - rect.y - rect.height;
  // The reader draws into a window-sized target, so it shifts the position back to the target's.
  const outputs = [{ name: 'gl_FragCoord', components: 4, packed: `gl_FragCoord + vec4(${OFFSET}, 0.0, 0.0)` }, ...varyings];
  const offset = { name: OFFSET, type: gl.FLOAT_VEC2, value: [rect.x, glY] };
  const most = gl.getParameter(gl.MAX_DRAW_BUFFERS) as number;
  const read = new Map<string, Float32Array>();
  for (let start = 0; start < outputs.length; start += most) {
    const chunk = outputs.slice(start, start + most);
    const program = programFor(kept, readerSource(varyings, chunk));
    if (typeof program === 'string') return `the input reader didn't link: ${program}`;
    const result = drawKept(kept, program, { x: rect.x, glY, width: rect.width, height: rect.height }, chunk.length, [offset]);
    chunk.forEach((o, i) => read.set(o.name, result[i]!));
  }
  const pack = (name: string, components: number) => {
    const all = flipRows(read.get(name)!, rect.width, rect.height, 4);
    const kept = new Float32Array(rect.width * rect.height * components);
    for (let p = 0; p < rect.width * rect.height; p++) for (let c = 0; c < components; c++) kept[p * components + c] = all[p * 4 + c]!;
    return toBase64(new Uint8Array(kept.buffer));
  };
  return {
    kind: 'window',
    rect,
    fragCoord: pack('gl_FragCoord', 4),
    varyings: varyings.map((v) => ({ name: v.name, components: v.components, data: pack(v.name, v.components) })),
  };
}

/** A fragment shader that writes `outputs` instead of a color. */
function readerSource(varyings: Varying[], outputs: { packed: string }[]): string {
  return [
    '#version 300 es',
    'precision highp float;',
    'precision highp int;',
    `uniform vec2 ${OFFSET};`,
    ...varyings.map((v) => v.declaration),
    ...outputs.map((_, i) => `layout(location = ${i}) out vec4 texelscope_out${i};`),
    'void main() {',
    ...outputs.map((o, i) => `  texelscope_out${i} = ${o.packed};`),
    '}',
  ].join('\n');
}
