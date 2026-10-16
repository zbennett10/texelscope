// What a captured draw used, kept so the page can draw it again later with another fragment
// shader: a what-if, a value to show over the image, or a reader of its inputs. Uniforms,
// uniform blocks and render targets are copied as they were, since later frames change them.
// The draw reads its vertices and other textures from the app's own buffers and textures.

import { originalDraws } from './install.js';
import { linkWith, readUniforms, samplerTarget, type UniformValue, writeUniforms } from './program.js';
import { type DrawCall, owned, programShaders, renderTargets, shaderSources, textures } from './state.js';

/** A vertex attribute read as one value for every vertex. */
interface Constant {
  location: number;
  value: Float32Array | Int32Array | Uint32Array;
}

/** A texture unit the draw sampled. */
interface Unit {
  unit: number;
  target: number;
  texture: WebGLTexture | null;
  sampler: WebGLSampler | null;
}

/** A uniform block, with a copy of the bytes it read. */
interface Block {
  name: string;
  point: number;
  buffer: WebGLBuffer;
}

export interface KeptDraw {
  readonly id: number;
  readonly gl: WebGL2RenderingContext;
  /** The app's program, whose vertex shader every program for this draw links with. */
  readonly program: WebGLProgram;
  readonly vertex: WebGLShader;
  /** The fragment shader's source, as the app gave it. */
  readonly source: string;
  readonly call: DrawCall;
  /** The size of the target the draw wrote to. */
  readonly width: number;
  readonly height: number;
  readonly vertexArray: WebGLVertexArrayObject;
  readonly constants: Constant[];
  readonly uniforms: UniformValue[];
  readonly blocks: Block[];
  readonly units: Unit[];
  readonly viewport: number[];
  readonly scissor: number[] | null;
  readonly cull: { mode: number; front: number } | null;
  readonly depth: { func: number; write: boolean; clear: number } | null;
  /** Programs linked for this draw by fragment source, or why one didn't link. */
  readonly programs: Map<string, WebGLProgram | string>;
  /** What the hook made for this draw, deleted with it. */
  readonly made: { textures: WebGLTexture[]; buffers: WebGLBuffer[] };
}

/** Keeps what the draw that just ran used, while its state is still bound. */
export function keepDraw(gl: WebGL2RenderingContext, call: DrawCall, id: number, width: number, height: number): KeptDraw | string {
  return owned(() => {
    const program = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
    const stages = program && programShaders.get(program);
    const source = stages?.fragment && shaderSources.get(stages.fragment);
    if (!program || !stages?.vertex || !source) return 'the program was made before Texelscope was installed';
    const made = { textures: [] as WebGLTexture[], buffers: [] as WebGLBuffer[] };
    const { vertexArray, constants } = keepVertices(gl, program);
    const depthOn = gl.isEnabled(gl.DEPTH_TEST) && targetHasDepth(gl);
    return {
      id,
      gl,
      program,
      vertex: stages.vertex,
      source,
      call: { method: call.method, args: [...call.args] },
      width,
      height,
      vertexArray,
      constants,
      uniforms: readUniforms(gl, program),
      blocks: keepBlocks(gl, program, made),
      units: keepUnits(gl, program, made),
      viewport: Array.from(gl.getParameter(gl.VIEWPORT) as Int32Array),
      scissor: gl.isEnabled(gl.SCISSOR_TEST) ? Array.from(gl.getParameter(gl.SCISSOR_BOX) as Int32Array) : null,
      cull: gl.isEnabled(gl.CULL_FACE) ? { mode: gl.getParameter(gl.CULL_FACE_MODE), front: gl.getParameter(gl.FRONT_FACE) } : null,
      depth: depthOn
        ? { func: gl.getParameter(gl.DEPTH_FUNC), write: gl.getParameter(gl.DEPTH_WRITEMASK), clear: gl.getParameter(gl.DEPTH_CLEAR_VALUE) }
        : null,
      programs: new Map(),
      made,
    };
  });
}

/** Deletes what the hook made for a kept draw. */
export function releaseDraw(kept: KeptDraw) {
  const gl = kept.gl;
  if (gl.isContextLost()) return;
  gl.deleteVertexArray(kept.vertexArray);
  for (const t of kept.made.textures) gl.deleteTexture(t);
  for (const b of kept.made.buffers) gl.deleteBuffer(b);
  for (const p of kept.programs.values()) if (typeof p !== 'string') gl.deleteProgram(p);
}

/** Where a pass's targets sit in the draw's target, in WebGL's window coordinates. */
export interface PassRect {
  x: number;
  glY: number;
  width: number;
  height: number;
}

/**
 * Draws the kept draw again with `program`, into `outputs` float targets the size of `rect`
 * that start as NaN, and reads each one back, bottom row first. `extra` sets uniforms of the
 * hook's own. Every piece of WebGL state the app had stays as it was.
 */
export function drawKept(kept: KeptDraw, program: WebGLProgram, rect: PassRect, outputs: number, extra: UniformValue[] = []): Float32Array[] {
  const gl = kept.gl;
  return owned(() => {
    const saved = save(gl, kept);
    const framebuffer = gl.createFramebuffer();
    const targets: WebGLTexture[] = [];
    let depth: WebGLRenderbuffer | null = null;
    try {
      gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
      for (let i = 0; i < outputs; i++) {
        const texture = gl.createTexture()!;
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA32F, rect.width, rect.height);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, texture, 0);
        targets.push(texture);
      }
      if (kept.depth) {
        depth = gl.createRenderbuffer();
        gl.bindRenderbuffer(gl.RENDERBUFFER, depth);
        gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT24, rect.width, rect.height);
        gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, depth);
      }
      gl.drawBuffers(targets.map((_, i) => gl.COLOR_ATTACHMENT0 + i));
      gl.useProgram(program);
      writeUniforms(gl, program, [...kept.uniforms, ...extra]);
      for (const block of kept.blocks) {
        const index = gl.getUniformBlockIndex(program, block.name);
        if (index === gl.INVALID_INDEX) continue;
        gl.uniformBlockBinding(program, index, block.point);
        gl.bindBufferBase(gl.UNIFORM_BUFFER, block.point, block.buffer);
      }
      for (const u of kept.units) {
        gl.activeTexture(gl.TEXTURE0 + u.unit);
        gl.bindTexture(u.target, u.texture);
        gl.bindSampler(u.unit, u.sampler);
      }
      gl.bindVertexArray(kept.vertexArray);
      for (const c of kept.constants) setConstant(gl, c.location, c.value);
      // Moving the viewport by the pass's corner lands that part of the target on its own.
      const [vx, vy, vw, vh] = kept.viewport as [number, number, number, number];
      gl.viewport(vx - rect.x, vy - rect.glY, vw, vh);
      if (kept.scissor) {
        const [sx, sy, sw, sh] = kept.scissor as [number, number, number, number];
        gl.enable(gl.SCISSOR_TEST);
        gl.scissor(sx - rect.x, sy - rect.glY, sw, sh);
      } else gl.disable(gl.SCISSOR_TEST);
      if (kept.cull) {
        gl.enable(gl.CULL_FACE);
        gl.cullFace(kept.cull.mode);
        gl.frontFace(kept.cull.front);
      } else gl.disable(gl.CULL_FACE);
      for (const cap of [gl.BLEND, gl.STENCIL_TEST, gl.RASTERIZER_DISCARD, gl.POLYGON_OFFSET_FILL, gl.SAMPLE_ALPHA_TO_COVERAGE, gl.SAMPLE_COVERAGE]) gl.disable(cap);
      gl.colorMask(true, true, true, true);
      for (let i = 0; i < outputs; i++) gl.clearBufferfv(gl.COLOR, i, [NaN, NaN, NaN, NaN]);
      if (kept.depth) {
        // The draw hides its own far side, since the app's depth buffer can't be copied.
        gl.enable(gl.DEPTH_TEST);
        gl.depthMask(true);
        gl.clearBufferfv(gl.DEPTH, 0, [kept.depth.clear]);
        gl.depthFunc(freshDepthFunc(gl, kept.depth.func, kept.depth.clear));
        gl.depthMask(kept.depth.write);
      } else gl.disable(gl.DEPTH_TEST);
      originalDraws[kept.call.method]!.apply(gl, kept.call.args);
      gl.bindBuffer(gl.PIXEL_PACK_BUFFER, null);
      gl.pixelStorei(gl.PACK_ROW_LENGTH, 0);
      gl.pixelStorei(gl.PACK_SKIP_PIXELS, 0);
      gl.pixelStorei(gl.PACK_SKIP_ROWS, 0);
      return targets.map((_, i) => {
        gl.readBuffer(gl.COLOR_ATTACHMENT0 + i);
        const data = new Float32Array(rect.width * rect.height * 4);
        gl.readPixels(0, 0, rect.width, rect.height, gl.RGBA, gl.FLOAT, data);
        return data;
      });
    } finally {
      restore(gl, saved);
      gl.deleteFramebuffer(framebuffer);
      for (const t of targets) gl.deleteTexture(t);
      if (depth) gl.deleteRenderbuffer(depth);
    }
  });
}

/** The program for this draw with `source` as its fragment shader, linked once per source. */
export function programFor(kept: KeptDraw, source: string): WebGLProgram | string {
  const known = kept.programs.get(source);
  if (known !== undefined) return known;
  const linked = owned(() => linkWith(kept.gl, kept.program, kept.vertex, source));
  // Keep a few, since an edit comes back to sources it had before.
  if (kept.programs.size >= 8) {
    const [oldest, program] = kept.programs.entries().next().value!;
    if (typeof program !== 'string') kept.gl.deleteProgram(program);
    kept.programs.delete(oldest);
  }
  kept.programs.set(source, linked);
  return linked;
}

/** A depth test against a cleared buffer that still lets the nearest fragment through. */
function freshDepthFunc(gl: WebGL2RenderingContext, func: number, clear: number): number {
  // An equal test after a depth prepass would pass nothing on a cleared buffer.
  if (func === gl.EQUAL) return clear >= 0.5 ? gl.LEQUAL : gl.GEQUAL;
  return func;
}

/** Whether the bound draw framebuffer has a depth buffer for a depth test to use. */
function targetHasDepth(gl: WebGL2RenderingContext): boolean {
  const framebuffer = gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  if (!framebuffer) return gl.getContextAttributes()?.depth !== false;
  const kind = gl.getFramebufferAttachmentParameter(gl.DRAW_FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE) as number;
  return kind !== gl.NONE;
}

/** How many attribute locations one element of an attribute type takes. */
function slots(gl: WebGL2RenderingContext, type: number): number {
  switch (type) {
    case gl.FLOAT_MAT2:
    case gl.FLOAT_MAT2x3:
    case gl.FLOAT_MAT2x4:
      return 2;
    case gl.FLOAT_MAT3:
    case gl.FLOAT_MAT3x2:
    case gl.FLOAT_MAT3x4:
      return 3;
    case gl.FLOAT_MAT4:
    case gl.FLOAT_MAT4x2:
    case gl.FLOAT_MAT4x3:
      return 4;
    default:
      return 1;
  }
}

/** A vertex array of the hook's own that points where the app's did for the program's attributes. */
function keepVertices(gl: WebGL2RenderingContext, program: WebGLProgram) {
  const pointers: { location: number; buffer: WebGLBuffer; size: number; type: number; normalized: boolean; stride: number; offset: number; integer: boolean; divisor: number }[] = [];
  const constants: Constant[] = [];
  const count = gl.getProgramParameter(program, gl.ACTIVE_ATTRIBUTES) as number;
  for (let i = 0; i < count; i++) {
    const info = gl.getActiveAttrib(program, i);
    if (!info || info.name.startsWith('gl_')) continue;
    const first = gl.getAttribLocation(program, info.name);
    if (first < 0) continue;
    for (let location = first; location < first + slots(gl, info.type) * info.size; location++) {
      const at = (name: number) => gl.getVertexAttrib(location, name);
      const buffer = at(gl.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING) as WebGLBuffer | null;
      if (at(gl.VERTEX_ATTRIB_ARRAY_ENABLED) && buffer) {
        pointers.push({
          location,
          buffer,
          size: at(gl.VERTEX_ATTRIB_ARRAY_SIZE),
          type: at(gl.VERTEX_ATTRIB_ARRAY_TYPE),
          normalized: at(gl.VERTEX_ATTRIB_ARRAY_NORMALIZED),
          stride: at(gl.VERTEX_ATTRIB_ARRAY_STRIDE),
          offset: gl.getVertexAttribOffset(location, gl.VERTEX_ATTRIB_ARRAY_POINTER),
          integer: at(gl.VERTEX_ATTRIB_ARRAY_INTEGER),
          divisor: at(gl.VERTEX_ATTRIB_ARRAY_DIVISOR),
        });
      } else {
        constants.push({ location, value: (at(gl.CURRENT_VERTEX_ATTRIB) as Float32Array).slice() });
      }
    }
  }
  const elements = gl.getParameter(gl.ELEMENT_ARRAY_BUFFER_BINDING) as WebGLBuffer | null;
  const savedArray = gl.getParameter(gl.VERTEX_ARRAY_BINDING) as WebGLVertexArrayObject | null;
  const savedBuffer = gl.getParameter(gl.ARRAY_BUFFER_BINDING) as WebGLBuffer | null;
  const vertexArray = gl.createVertexArray()!;
  gl.bindVertexArray(vertexArray);
  for (const p of pointers) {
    gl.bindBuffer(gl.ARRAY_BUFFER, p.buffer);
    if (p.integer) gl.vertexAttribIPointer(p.location, p.size, p.type, p.stride, p.offset);
    else gl.vertexAttribPointer(p.location, p.size, p.type, p.normalized, p.stride, p.offset);
    gl.enableVertexAttribArray(p.location);
    gl.vertexAttribDivisor(p.location, p.divisor);
  }
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, elements);
  gl.bindVertexArray(savedArray);
  gl.bindBuffer(gl.ARRAY_BUFFER, savedBuffer);
  return { vertexArray, constants };
}

function setConstant(gl: WebGL2RenderingContext, location: number, value: Float32Array | Int32Array | Uint32Array) {
  if (value instanceof Int32Array) gl.vertexAttribI4iv(location, value);
  else if (value instanceof Uint32Array) gl.vertexAttribI4uiv(location, value);
  else gl.vertexAttrib4fv(location, value);
}

/** Copies of the bytes each uniform block read, made on the GPU. */
function keepBlocks(gl: WebGL2RenderingContext, program: WebGLProgram, made: KeptDraw['made']): Block[] {
  const out: Block[] = [];
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORM_BLOCKS) as number;
  const savedUniform = gl.getParameter(gl.UNIFORM_BUFFER_BINDING) as WebGLBuffer | null;
  const savedRead = gl.getParameter(gl.COPY_READ_BUFFER_BINDING) as WebGLBuffer | null;
  for (let i = 0; i < count; i++) {
    const name = gl.getActiveUniformBlockName(program, i);
    const point = gl.getActiveUniformBlockParameter(program, i, gl.UNIFORM_BLOCK_BINDING) as number;
    const size = gl.getActiveUniformBlockParameter(program, i, gl.UNIFORM_BLOCK_DATA_SIZE) as number;
    const bound = gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING, point) as WebGLBuffer | null;
    if (!name || !bound) continue;
    const start = gl.getIndexedParameter(gl.UNIFORM_BUFFER_START, point) as number;
    const buffer = gl.createBuffer()!;
    gl.bindBuffer(gl.UNIFORM_BUFFER, buffer);
    gl.bufferData(gl.UNIFORM_BUFFER, size, gl.STATIC_DRAW);
    gl.bindBuffer(gl.COPY_READ_BUFFER, bound);
    gl.copyBufferSubData(gl.COPY_READ_BUFFER, gl.UNIFORM_BUFFER, start, 0, size);
    made.buffers.push(buffer);
    out.push({ name, point, buffer });
  }
  gl.bindBuffer(gl.UNIFORM_BUFFER, savedUniform);
  gl.bindBuffer(gl.COPY_READ_BUFFER, savedRead);
  return out;
}

/** The textures and samplers on every unit a sampler uniform names, with render targets copied. */
function keepUnits(gl: WebGL2RenderingContext, program: WebGLProgram, made: KeptDraw['made']): Unit[] {
  const out: Unit[] = [];
  const savedUnit = gl.getParameter(gl.ACTIVE_TEXTURE) as number;
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS) as number;
  for (let i = 0; i < count; i++) {
    const info = gl.getActiveUniform(program, i);
    const kind = info && samplerTarget(gl, info.type);
    if (!info || !kind) continue;
    const [target, binding] = kind;
    const base = info.name.replace(/\[0\]$/, '');
    for (let k = 0; k < info.size; k++) {
      const location = gl.getUniformLocation(program, info.size > 1 ? `${base}[${k}]` : info.name);
      if (!location) continue;
      const unit = gl.getUniform(program, location) as number;
      if (out.some((u) => u.unit === unit && u.target === target)) continue;
      gl.activeTexture(gl.TEXTURE0 + unit);
      const texture = gl.getParameter(binding) as WebGLTexture | null;
      const sampler = gl.getParameter(gl.SAMPLER_BINDING) as WebGLSampler | null;
      const copy = texture && target === gl.TEXTURE_2D && renderTargets.has(texture) ? copyTexture(gl, texture, made) : undefined;
      out.push({ unit, target, texture: copy ?? texture, sampler });
    }
  }
  gl.activeTexture(savedUnit);
  return out;
}

/** The sized format an internal format and upload type make, for formats a copy can hold. */
function sizedFormat(gl: WebGL2RenderingContext, internal: number, type: number | undefined): number | undefined {
  if (internal === gl.RGBA) return type === gl.UNSIGNED_BYTE ? gl.RGBA8 : undefined;
  if (internal === gl.RGB) return type === gl.UNSIGNED_BYTE ? gl.RGB8 : undefined;
  if (internal === gl.DEPTH_COMPONENT) return type === gl.UNSIGNED_INT ? gl.DEPTH_COMPONENT24 : type === gl.UNSIGNED_SHORT ? gl.DEPTH_COMPONENT16 : undefined;
  if (internal === gl.DEPTH_STENCIL) return type === gl.UNSIGNED_INT_24_8 ? gl.DEPTH24_STENCIL8 : undefined;
  if (([gl.LUMINANCE, gl.LUMINANCE_ALPHA, gl.ALPHA] as number[]).includes(internal)) return undefined;
  return internal;
}

const DEPTH_FORMATS = (gl: WebGL2RenderingContext): number[] => [gl.DEPTH_COMPONENT16, gl.DEPTH_COMPONENT24, gl.DEPTH_COMPONENT32F];
const DEPTH_STENCIL_FORMATS = (gl: WebGL2RenderingContext): number[] => [gl.DEPTH24_STENCIL8, gl.DEPTH32F_STENCIL8];

/**
 * Copies every level of a 2D texture on the GPU, with its sampling parameters, while it is
 * bound to the active unit. Returns undefined when the copy can't be made, and the draw then
 * reads the app's texture as it is when it runs again.
 */
function copyTexture(gl: WebGL2RenderingContext, texture: WebGLTexture, made: KeptDraw['made']): WebGLTexture | undefined {
  const info = textures.get(texture);
  const format = info && info.target === gl.TEXTURE_2D ? sizedFormat(gl, info.internalFormat, info.type) : undefined;
  if (!info || !format || !info.levels.has(0)) return undefined;
  let levels = 0;
  while (info.levels.has(levels)) levels++;
  const depth = DEPTH_FORMATS(gl).includes(format);
  const depthStencil = DEPTH_STENCIL_FORMATS(gl).includes(format);
  const attachment = depthStencil ? gl.DEPTH_STENCIL_ATTACHMENT : depth ? gl.DEPTH_ATTACHMENT : gl.COLOR_ATTACHMENT0;
  const mask = depthStencil ? gl.DEPTH_BUFFER_BIT | gl.STENCIL_BUFFER_BIT : depth ? gl.DEPTH_BUFFER_BIT : gl.COLOR_BUFFER_BIT;
  if (!depth && !depthStencil && isFloat(gl, format)) gl.getExtension('EXT_color_buffer_float');

  const ints: number[] = [gl.TEXTURE_MIN_FILTER, gl.TEXTURE_MAG_FILTER, gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T, gl.TEXTURE_WRAP_R, gl.TEXTURE_COMPARE_MODE, gl.TEXTURE_COMPARE_FUNC, gl.TEXTURE_BASE_LEVEL, gl.TEXTURE_MAX_LEVEL];
  const floats: number[] = [gl.TEXTURE_MIN_LOD, gl.TEXTURE_MAX_LOD];
  const params = [...ints, ...floats].map((p) => [p, gl.getTexParameter(gl.TEXTURE_2D, p) as number] as const);
  const [width, height] = info.levels.get(0)!;
  const copy = gl.createTexture()!;
  gl.bindTexture(gl.TEXTURE_2D, copy);
  gl.texStorage2D(gl.TEXTURE_2D, levels, format, width, height);
  for (const [p, v] of params) {
    if (floats.includes(p)) gl.texParameterf(gl.TEXTURE_2D, p, v);
    else gl.texParameteri(gl.TEXTURE_2D, p, v);
  }

  const savedRead = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  const savedDraw = gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  const scissor = gl.isEnabled(gl.SCISSOR_TEST);
  const discard = gl.isEnabled(gl.RASTERIZER_DISCARD);
  gl.disable(gl.SCISSOR_TEST);
  gl.disable(gl.RASTERIZER_DISCARD);
  const read = gl.createFramebuffer();
  const draw = gl.createFramebuffer();
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, read);
  gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, draw);
  let copied = true;
  for (let level = 0; level < levels && copied; level++) {
    const [w, h] = info.levels.get(level)!;
    gl.framebufferTexture2D(gl.READ_FRAMEBUFFER, attachment, gl.TEXTURE_2D, texture, level);
    gl.framebufferTexture2D(gl.DRAW_FRAMEBUFFER, attachment, gl.TEXTURE_2D, copy, level);
    copied = gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE && gl.checkFramebufferStatus(gl.DRAW_FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
    if (copied) gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, mask, gl.NEAREST);
  }
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, savedRead);
  gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, savedDraw);
  gl.deleteFramebuffer(read);
  gl.deleteFramebuffer(draw);
  if (scissor) gl.enable(gl.SCISSOR_TEST);
  if (discard) gl.enable(gl.RASTERIZER_DISCARD);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  if (!copied) {
    gl.deleteTexture(copy);
    return undefined;
  }
  made.textures.push(copy);
  return copy;
}

function isFloat(gl: WebGL2RenderingContext, format: number): boolean {
  return ([gl.R16F, gl.RG16F, gl.RGBA16F, gl.R32F, gl.RG32F, gl.RGBA32F, gl.R11F_G11F_B10F] as number[]).includes(format);
}

/** Every piece of state a pass over a kept draw changes. */
interface Saved {
  program: WebGLProgram | null;
  draw: WebGLFramebuffer | null;
  read: WebGLFramebuffer | null;
  renderbuffer: WebGLRenderbuffer | null;
  vertexArray: WebGLVertexArrayObject | null;
  viewport: Int32Array;
  scissorBox: Int32Array;
  enabled: [number, boolean][];
  cullMode: number;
  frontFace: number;
  depthFunc: number;
  depthMask: boolean;
  colorMask: boolean[];
  activeTexture: number;
  units: { unit: number; textures: (WebGLTexture | null)[]; sampler: WebGLSampler | null }[];
  uniformBuffer: WebGLBuffer | null;
  points: { point: number; buffer: WebGLBuffer | null; start: number; size: number }[];
  constants: { location: number; value: Float32Array | Int32Array | Uint32Array }[];
  packBuffer: WebGLBuffer | null;
  pack: [number, number][];
}

const CAPS = (gl: WebGL2RenderingContext) => [gl.SCISSOR_TEST, gl.CULL_FACE, gl.DEPTH_TEST, gl.BLEND, gl.STENCIL_TEST, gl.RASTERIZER_DISCARD, gl.POLYGON_OFFSET_FILL, gl.SAMPLE_ALPHA_TO_COVERAGE, gl.SAMPLE_COVERAGE];
const TARGETS = (gl: WebGL2RenderingContext) => [
  [gl.TEXTURE_2D, gl.TEXTURE_BINDING_2D],
  [gl.TEXTURE_3D, gl.TEXTURE_BINDING_3D],
  [gl.TEXTURE_CUBE_MAP, gl.TEXTURE_BINDING_CUBE_MAP],
  [gl.TEXTURE_2D_ARRAY, gl.TEXTURE_BINDING_2D_ARRAY],
] as const;

function save(gl: WebGL2RenderingContext, kept: KeptDraw): Saved {
  const activeTexture = gl.getParameter(gl.ACTIVE_TEXTURE) as number;
  const units = [...new Set([activeTexture - gl.TEXTURE0, ...kept.units.map((u) => u.unit)])].map((unit) => {
    gl.activeTexture(gl.TEXTURE0 + unit);
    return {
      unit,
      textures: TARGETS(gl).map(([, binding]) => gl.getParameter(binding) as WebGLTexture | null),
      sampler: gl.getParameter(gl.SAMPLER_BINDING) as WebGLSampler | null,
    };
  });
  gl.activeTexture(activeTexture);
  return {
    program: gl.getParameter(gl.CURRENT_PROGRAM),
    draw: gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING),
    read: gl.getParameter(gl.READ_FRAMEBUFFER_BINDING),
    renderbuffer: gl.getParameter(gl.RENDERBUFFER_BINDING),
    vertexArray: gl.getParameter(gl.VERTEX_ARRAY_BINDING),
    viewport: gl.getParameter(gl.VIEWPORT),
    scissorBox: gl.getParameter(gl.SCISSOR_BOX),
    enabled: CAPS(gl).map((cap) => [cap, gl.isEnabled(cap)]),
    cullMode: gl.getParameter(gl.CULL_FACE_MODE),
    frontFace: gl.getParameter(gl.FRONT_FACE),
    depthFunc: gl.getParameter(gl.DEPTH_FUNC),
    depthMask: gl.getParameter(gl.DEPTH_WRITEMASK),
    colorMask: gl.getParameter(gl.COLOR_WRITEMASK),
    activeTexture,
    units,
    uniformBuffer: gl.getParameter(gl.UNIFORM_BUFFER_BINDING),
    points: kept.blocks.map(({ point }) => ({
      point,
      buffer: gl.getIndexedParameter(gl.UNIFORM_BUFFER_BINDING, point),
      start: gl.getIndexedParameter(gl.UNIFORM_BUFFER_START, point),
      size: gl.getIndexedParameter(gl.UNIFORM_BUFFER_SIZE, point),
    })),
    constants: kept.constants.map(({ location }) => ({ location, value: gl.getVertexAttrib(location, gl.CURRENT_VERTEX_ATTRIB) })),
    packBuffer: gl.getParameter(gl.PIXEL_PACK_BUFFER_BINDING),
    pack: [gl.PACK_ROW_LENGTH, gl.PACK_SKIP_PIXELS, gl.PACK_SKIP_ROWS].map((p) => [p, gl.getParameter(p) as number]),
  };
}

function restore(gl: WebGL2RenderingContext, s: Saved) {
  gl.useProgram(s.program);
  gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, s.draw);
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, s.read);
  gl.bindRenderbuffer(gl.RENDERBUFFER, s.renderbuffer);
  gl.bindVertexArray(s.vertexArray);
  gl.viewport(s.viewport[0]!, s.viewport[1]!, s.viewport[2]!, s.viewport[3]!);
  gl.scissor(s.scissorBox[0]!, s.scissorBox[1]!, s.scissorBox[2]!, s.scissorBox[3]!);
  for (const [cap, on] of s.enabled) {
    if (on) gl.enable(cap);
    else gl.disable(cap);
  }
  gl.cullFace(s.cullMode);
  gl.frontFace(s.frontFace);
  gl.depthFunc(s.depthFunc);
  gl.depthMask(s.depthMask);
  gl.colorMask(s.colorMask[0]!, s.colorMask[1]!, s.colorMask[2]!, s.colorMask[3]!);
  for (const u of s.units) {
    gl.activeTexture(gl.TEXTURE0 + u.unit);
    TARGETS(gl).forEach(([target], i) => gl.bindTexture(target, u.textures[i]!));
    gl.bindSampler(u.unit, u.sampler);
  }
  gl.activeTexture(s.activeTexture);
  for (const p of s.points) {
    if (p.buffer && p.size > 0) gl.bindBufferRange(gl.UNIFORM_BUFFER, p.point, p.buffer, p.start, p.size);
    else gl.bindBufferBase(gl.UNIFORM_BUFFER, p.point, p.buffer);
  }
  gl.bindBuffer(gl.UNIFORM_BUFFER, s.uniformBuffer);
  for (const c of s.constants) setConstant(gl, c.location, c.value);
  gl.bindBuffer(gl.PIXEL_PACK_BUFFER, s.packBuffer);
  for (const [p, v] of s.pack) gl.pixelStorei(p, v);
}
