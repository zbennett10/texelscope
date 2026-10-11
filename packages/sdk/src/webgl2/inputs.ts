// Reads the inputs a draw gave each pixel of a window, by issuing the draw again with a
// fragment shader that writes them out instead of a color.

import type { FragmentInputs, Rect } from '../capture.js';
import { toBase64 } from '../encode.js';
import { originalDraws } from './install.js';
import { type DrawTarget, flipRows } from './read.js';
import { type DrawCall, programShaders } from './state.js';

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
  const pattern = /(?:^|[;}])\s*((?:layout\s*\([^)]*\)\s*)?(?:(?:flat|smooth|noperspective|centroid|highp|mediump|lowp)\s+)*)in\s+((?:(?:highp|mediump|lowp)\s+)?)(\w+)\s+(\w+)\s*;/g;
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

/** Reads the fragment position and every declared input over `rect`, a window of the target. */
export function windowInputs(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  call: DrawCall,
  fragmentSource: string,
  target: DrawTarget,
  rect: Rect,
): FragmentInputs | string {
  const vertex = programShaders.get(program)?.vertex;
  if (!vertex) return "the program's vertex shader was attached before Texelscope was installed";
  if (!gl.getExtension('EXT_color_buffer_float')) return 'this GPU cannot render to float targets, so the inputs cannot be read';
  const varyings = declaredInputs(fragmentSource);
  const glY = target.height - rect.y - rect.height;
  // The reader draws into a window-sized target, so it shifts the position back to the target's.
  const outputs = [
    { name: 'gl_FragCoord', components: 4, packed: `gl_FragCoord + vec4(${rect.x.toFixed(1)}, ${glY.toFixed(1)}, 0.0, 0.0)` },
    ...varyings,
  ];
  const most = gl.getParameter(gl.MAX_DRAW_BUFFERS) as number;
  const read = new Map<string, Float32Array>();
  for (let start = 0; start < outputs.length; start += most) {
    const chunk = outputs.slice(start, start + most);
    const result = drawOutputs(gl, program, vertex, call, varyings, chunk, rect, glY);
    if (typeof result === 'string') return result;
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

/** Draws the call again with a reader that writes `outputs`, and reads each one back. */
function drawOutputs(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  vertex: WebGLShader,
  call: DrawCall,
  varyings: Varying[],
  outputs: { name: string; packed: string }[],
  rect: Rect,
  glY: number,
): Float32Array[] | string {
  const source = [
    '#version 300 es',
    'precision highp float;',
    'precision highp int;',
    ...varyings.map((v) => v.declaration),
    ...outputs.map((_, i) => `layout(location = ${i}) out vec4 texelscope_out${i};`),
    'void main() {',
    ...outputs.map((o, i) => `  texelscope_out${i} = ${o.packed};`),
    '}',
  ].join('\n');
  const fragment = gl.createShader(gl.FRAGMENT_SHADER)!;
  gl.shaderSource(fragment, source);
  gl.compileShader(fragment);
  const reader = gl.createProgram()!;
  const cleanup = () => {
    gl.deleteProgram(reader);
    gl.deleteShader(fragment);
  };
  if (!gl.getShaderParameter(fragment, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(fragment);
    cleanup();
    return `the input reader didn't compile: ${log}`;
  }
  gl.attachShader(reader, vertex);
  gl.attachShader(reader, fragment);
  // Attributes keep the locations the app's program gave them, so its vertex arrays still fit.
  const attributes = gl.getProgramParameter(program, gl.ACTIVE_ATTRIBUTES) as number;
  for (let i = 0; i < attributes; i++) {
    const info = gl.getActiveAttrib(program, i);
    if (!info || info.name.startsWith('gl_')) continue;
    const location = gl.getAttribLocation(program, info.name);
    if (location >= 0) gl.bindAttribLocation(reader, location, info.name);
  }
  gl.linkProgram(reader);
  if (!gl.getProgramParameter(reader, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(reader);
    cleanup();
    return `the input reader didn't link: ${log}`;
  }

  const saved = saveState(gl);
  const framebuffer = gl.createFramebuffer();
  const targets: WebGLTexture[] = [];
  try {
    gl.useProgram(reader);
    copyUniforms(gl, program, reader);
    gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
    for (let i = 0; i < outputs.length; i++) {
      const texture = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA32F, rect.width, rect.height);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, texture, 0);
      targets.push(texture);
    }
    gl.drawBuffers(outputs.map((_, i) => gl.COLOR_ATTACHMENT0 + i));
    // Moving the viewport by the window's corner lands the window on the small target.
    const [vx, vy, vw, vh] = saved.viewport;
    gl.viewport(vx! - rect.x, vy! - glY, vw!, vh!);
    for (const cap of [gl.SCISSOR_TEST, gl.BLEND, gl.DEPTH_TEST, gl.STENCIL_TEST, gl.RASTERIZER_DISCARD]) gl.disable(cap);
    gl.colorMask(true, true, true, true);
    for (let i = 0; i < outputs.length; i++) gl.clearBufferfv(gl.COLOR, i, [NaN, NaN, NaN, NaN]);
    originalDraws[call.method]!.apply(gl, call.args);
    const out: Float32Array[] = [];
    for (let i = 0; i < outputs.length; i++) {
      gl.readBuffer(gl.COLOR_ATTACHMENT0 + i);
      const data = new Float32Array(rect.width * rect.height * 4);
      gl.readPixels(0, 0, rect.width, rect.height, gl.RGBA, gl.FLOAT, data);
      out.push(data);
    }
    return out;
  } finally {
    restoreState(gl, saved);
    gl.deleteFramebuffer(framebuffer);
    for (const t of targets) gl.deleteTexture(t);
    cleanup();
  }
}

interface Saved {
  program: WebGLProgram | null;
  draw: WebGLFramebuffer | null;
  read: WebGLFramebuffer | null;
  viewport: Int32Array;
  enabled: [number, boolean][];
  colorMask: boolean[];
  texture: WebGLTexture | null;
}

function saveState(gl: WebGL2RenderingContext): Saved {
  return {
    program: gl.getParameter(gl.CURRENT_PROGRAM),
    draw: gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING),
    read: gl.getParameter(gl.READ_FRAMEBUFFER_BINDING),
    viewport: gl.getParameter(gl.VIEWPORT),
    enabled: [gl.SCISSOR_TEST, gl.BLEND, gl.DEPTH_TEST, gl.STENCIL_TEST, gl.RASTERIZER_DISCARD].map((cap) => [cap, gl.isEnabled(cap)]),
    colorMask: gl.getParameter(gl.COLOR_WRITEMASK),
    texture: gl.getParameter(gl.TEXTURE_BINDING_2D),
  };
}

function restoreState(gl: WebGL2RenderingContext, s: Saved) {
  gl.useProgram(s.program);
  gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, s.draw);
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, s.read);
  gl.viewport(s.viewport[0]!, s.viewport[1]!, s.viewport[2]!, s.viewport[3]!);
  for (const [cap, on] of s.enabled) {
    if (on) gl.enable(cap);
    else gl.disable(cap);
  }
  gl.colorMask(s.colorMask[0]!, s.colorMask[1]!, s.colorMask[2]!, s.colorMask[3]!);
  gl.bindTexture(gl.TEXTURE_2D, s.texture);
}

/** Gives a program the values another holds for the uniforms they share, and its block bindings. */
function copyUniforms(gl: WebGL2RenderingContext, from: WebGLProgram, to: WebGLProgram) {
  const count = gl.getProgramParameter(to, gl.ACTIVE_UNIFORMS) as number;
  for (let i = 0; i < count; i++) {
    const info = gl.getActiveUniform(to, i);
    if (!info) continue;
    const base = info.name.replace(/\[0\]$/, '');
    for (let k = 0; k < info.size; k++) {
      const name = info.size > 1 ? `${base}[${k}]` : info.name;
      const source = gl.getUniformLocation(from, name);
      const destination = gl.getUniformLocation(to, name);
      if (source && destination) setUniform(gl, destination, info.type, gl.getUniform(from, source));
    }
  }
  const blocks = gl.getProgramParameter(to, gl.ACTIVE_UNIFORM_BLOCKS) as number;
  for (let i = 0; i < blocks; i++) {
    const name = gl.getActiveUniformBlockName(to, i);
    const original = name ? gl.getUniformBlockIndex(from, name) : gl.INVALID_INDEX;
    if (original === gl.INVALID_INDEX) continue;
    gl.uniformBlockBinding(to, i, gl.getActiveUniformBlockParameter(from, original, gl.UNIFORM_BLOCK_BINDING) as number);
  }
}

/** Sets a uniform from the value getUniform gave for it. */
export function setUniform(gl: WebGL2RenderingContext, at: WebGLUniformLocation, type: number, value: unknown) {
  const floats = () => Float32Array.from(value as ArrayLike<number>);
  const ints = () => Int32Array.from(value as ArrayLike<number | boolean>, Number);
  const uints = () => Uint32Array.from(value as ArrayLike<number>);
  switch (type) {
    case gl.FLOAT: return gl.uniform1f(at, value as number);
    case gl.FLOAT_VEC2: return gl.uniform2fv(at, floats());
    case gl.FLOAT_VEC3: return gl.uniform3fv(at, floats());
    case gl.FLOAT_VEC4: return gl.uniform4fv(at, floats());
    case gl.INT_VEC2: case gl.BOOL_VEC2: return gl.uniform2iv(at, ints());
    case gl.INT_VEC3: case gl.BOOL_VEC3: return gl.uniform3iv(at, ints());
    case gl.INT_VEC4: case gl.BOOL_VEC4: return gl.uniform4iv(at, ints());
    case gl.UNSIGNED_INT: return gl.uniform1ui(at, value as number);
    case gl.UNSIGNED_INT_VEC2: return gl.uniform2uiv(at, uints());
    case gl.UNSIGNED_INT_VEC3: return gl.uniform3uiv(at, uints());
    case gl.UNSIGNED_INT_VEC4: return gl.uniform4uiv(at, uints());
    case gl.FLOAT_MAT2: return gl.uniformMatrix2fv(at, false, floats());
    case gl.FLOAT_MAT3: return gl.uniformMatrix3fv(at, false, floats());
    case gl.FLOAT_MAT4: return gl.uniformMatrix4fv(at, false, floats());
    case gl.FLOAT_MAT2x3: return gl.uniformMatrix2x3fv(at, false, floats());
    case gl.FLOAT_MAT2x4: return gl.uniformMatrix2x4fv(at, false, floats());
    case gl.FLOAT_MAT3x2: return gl.uniformMatrix3x2fv(at, false, floats());
    case gl.FLOAT_MAT3x4: return gl.uniformMatrix3x4fv(at, false, floats());
    case gl.FLOAT_MAT4x2: return gl.uniformMatrix4x2fv(at, false, floats());
    case gl.FLOAT_MAT4x3: return gl.uniformMatrix4x3fv(at, false, floats());
    // Ints, bools and samplers, whose value is a texture unit.
    default: return gl.uniform1i(at, Number(value));
  }
}
