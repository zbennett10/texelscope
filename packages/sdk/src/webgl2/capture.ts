// Builds a capture of the WebGL2 draw that just ran, from the state still bound for it.

import type { Binding, Capture, Rect } from '../capture.js';
import { toBase64 } from '../encode.js';
import { windowInputs } from './inputs.js';
import { type DrawTarget, drawTarget, readTexture, samplerState, targetBytes } from './read.js';
import { type DrawCall, programShaders, shaderSources } from './state.js';

/** How many pixels on a side the window of recorded inputs covers. */
export const WINDOW = 128;

export interface DrawCaptureOptions {
  /** The pixel to center the window of inputs on, from the top left. Defaults to the middle. */
  pixel?: [number, number];
  window?: number;
  label?: string;
  frame?: number;
}

/** Captures the draw that just ran on `gl`. Leaves every piece of WebGL state as it was. */
export function captureDraw(gl: WebGL2RenderingContext, call: DrawCall, options: DrawCaptureOptions = {}): Capture {
  const program = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
  if (!program) throw new Error('no program was bound for the draw');
  const fragment = programShaders.get(program)?.fragment;
  const source = fragment && shaderSources.get(fragment);
  if (!source) throw new Error('the program was made before Texelscope was installed, so its source is unknown');
  const target = drawTarget(gl);
  if (!target.readable || target.width === 0) throw new Error("the draw's target can't be read back");
  const rect = windowAround(target, options.pixel, options.window ?? WINDOW);
  const bindings = [...uniforms(gl, program), ...blocks(gl, program), ...samplers(gl, program)];
  const inputs = windowInputs(gl, program, call, source, target, rect);
  if (typeof inputs === 'string') throw new Error(inputs);
  return {
    version: 0,
    label: options.label,
    frame: options.frame,
    shader: { language: 'glsl', source: { inline: source }, entryPoint: 'main' },
    target: { width: target.width, height: target.height, format: target.float ? 'rgba32float' : 'rgba8unorm', yUp: true },
    inputs,
    bindings,
    reference: { rect: { x: 0, y: 0, width: target.width, height: target.height }, rgba8: toBase64(targetBytes(gl, target)) },
  };
}

/** A window of up to `size` pixels on a side around a pixel, inside the target. */
function windowAround(target: DrawTarget, pixel: [number, number] | undefined, size: number): Rect {
  const [px, py] = pixel ?? [Math.floor(target.width / 2), Math.floor(target.height / 2)];
  const width = Math.min(size, target.width);
  const height = Math.min(size, target.height);
  const x = Math.min(Math.max(0, px - Math.floor(width / 2)), target.width - width);
  const y = Math.min(Math.max(0, py - Math.floor(height / 2)), target.height - height);
  return { x, y, width, height };
}

interface ActiveUniform {
  info: WebGLActiveInfo;
  location: WebGLUniformLocation | null;
}

/** The program's uniforms outside blocks. */
function activeUniforms(gl: WebGL2RenderingContext, program: WebGLProgram): ActiveUniform[] {
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS) as number;
  if (count === 0) return [];
  const indices = [...Array(count).keys()];
  const blockOf = gl.getActiveUniforms(program, indices, gl.UNIFORM_BLOCK_INDEX) as number[];
  return indices.flatMap((i) => {
    const info = gl.getActiveUniform(program, i);
    if (!info || blockOf[i] !== -1) return [];
    return [{ info, location: gl.getUniformLocation(program, info.name) }];
  });
}

function isSampler(gl: WebGL2RenderingContext, type: number): boolean {
  return samplerTarget(gl, type) !== undefined;
}

/** The texture target and binding query for each sampler type. */
function samplerTarget(gl: WebGL2RenderingContext, type: number): [number, number] | undefined {
  switch (type) {
    case gl.SAMPLER_2D:
    case gl.INT_SAMPLER_2D:
    case gl.UNSIGNED_INT_SAMPLER_2D:
    case gl.SAMPLER_2D_SHADOW:
      return [gl.TEXTURE_2D, gl.TEXTURE_BINDING_2D];
    case gl.SAMPLER_3D:
    case gl.INT_SAMPLER_3D:
    case gl.UNSIGNED_INT_SAMPLER_3D:
      return [gl.TEXTURE_3D, gl.TEXTURE_BINDING_3D];
    case gl.SAMPLER_CUBE:
    case gl.INT_SAMPLER_CUBE:
    case gl.UNSIGNED_INT_SAMPLER_CUBE:
    case gl.SAMPLER_CUBE_SHADOW:
      return [gl.TEXTURE_CUBE_MAP, gl.TEXTURE_BINDING_CUBE_MAP];
    case gl.SAMPLER_2D_ARRAY:
    case gl.INT_SAMPLER_2D_ARRAY:
    case gl.UNSIGNED_INT_SAMPLER_2D_ARRAY:
    case gl.SAMPLER_2D_ARRAY_SHADOW:
      return [gl.TEXTURE_2D_ARRAY, gl.TEXTURE_BINDING_2D_ARRAY];
    default:
      return undefined;
  }
}

/** A typed array from getUniform as plain numbers and booleans. */
function plain(value: unknown): unknown {
  if (ArrayBuffer.isView(value)) return Array.from(value as unknown as ArrayLike<number>);
  return value;
}

/** Sets `value` at a uniform path such as `light.color` or `lights[1].color`. */
function setPath(root: Record<string, unknown>, path: string, value: unknown) {
  const parts = [...path.matchAll(/(\w+)|\[(\d+)\]/g)].map((m) => (m[1] !== undefined ? m[1] : Number(m[2])));
  let at: Record<string | number, unknown> = root;
  parts.forEach((part, i) => {
    if (i === parts.length - 1) {
      at[part] = value;
      return;
    }
    const next = parts[i + 1];
    at[part] ??= typeof next === 'number' ? [] : {};
    at = at[part] as Record<string | number, unknown>;
  });
}

/** Each uniform outside a block, by its top-level name, with its value as getUniform gives it. */
function uniforms(gl: WebGL2RenderingContext, program: WebGLProgram): Binding[] {
  const values: Record<string, unknown> = {};
  for (const { info } of activeUniforms(gl, program)) {
    if (isSampler(gl, info.type)) continue;
    const base = info.name.replace(/\[0\]$/, '');
    const read = (name: string) => {
      const location = gl.getUniformLocation(program, name);
      return location ? plain(gl.getUniform(program, location)) : undefined;
    };
    const value = info.size > 1 ? Array.from({ length: info.size }, (_, k) => read(`${base}[${k}]`)) : read(info.name);
    setPath(values, base, value);
  }
  return Object.entries(values).map(([name, value]) => ({
    group: 0,
    binding: 0,
    name,
    resource: { kind: 'uniform' as const, data: { values: { [name]: value } } },
  }));
}

/** Each uniform block, as the bytes of the buffer range bound to its binding point. */
function blocks(gl: WebGL2RenderingContext, program: WebGLProgram): Binding[] {
  const out: Binding[] = [];
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORM_BLOCKS) as number;
  for (let i = 0; i < count; i++) {
    const name = gl.getActiveUniformBlockName(program, i);
    const point = gl.getActiveUniformBlockParameter(program, i, gl.UNIFORM_BLOCK_BINDING) as number;
    const size = gl.getActiveUniformBlockParameter(program, i, gl.UNIFORM_BLOCK_DATA_SIZE) as number;
    const buffer = gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING, point) as WebGLBuffer | null;
    if (!name || !buffer) continue;
    const start = gl.getIndexedParameter(gl.UNIFORM_BUFFER_START, point) as number;
    const saved = gl.getParameter(gl.COPY_READ_BUFFER_BINDING) as WebGLBuffer | null;
    gl.bindBuffer(gl.COPY_READ_BUFFER, buffer);
    const bytes = new Uint8Array(size);
    gl.getBufferSubData(gl.COPY_READ_BUFFER, start, bytes);
    gl.bindBuffer(gl.COPY_READ_BUFFER, saved);
    out.push({
      group: 0,
      binding: 0,
      name: name.replace(/\[\d+\]$/, ''),
      resource: { kind: 'uniform', data: { base64: toBase64(bytes) } },
    });
  }
  return out;
}

/** Each sampler uniform's texture and how it is sampled, by the uniform's name. */
function samplers(gl: WebGL2RenderingContext, program: WebGLProgram): Binding[] {
  const out: Binding[] = [];
  for (const { info, location } of activeUniforms(gl, program)) {
    const kind = samplerTarget(gl, info.type);
    if (!kind || !location || info.size > 1) continue;
    const [target, binding] = kind;
    const unit = gl.getUniform(program, location) as number;
    const saved = gl.getParameter(gl.ACTIVE_TEXTURE) as number;
    gl.activeTexture(gl.TEXTURE0 + unit);
    const texture = gl.getParameter(binding) as WebGLTexture | null;
    // Parameters are read from the texture bound to this unit, so read them before switching back.
    const sampler = texture ? samplerState(gl, target, gl.getParameter(gl.SAMPLER_BINDING) as WebGLSampler | null) : undefined;
    gl.activeTexture(saved);
    if (!texture || !sampler) continue;
    const read = readTexture(gl, texture);
    if (typeof read !== 'string') out.push({ group: 0, binding: 0, name: info.name, resource: { kind: 'texture', ...read } });
    out.push({ group: 0, binding: 0, name: info.name, resource: { kind: 'sampler', ...sampler } });
  }
  return out;
}
