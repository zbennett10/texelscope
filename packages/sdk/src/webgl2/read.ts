// Reads what WebGL2 holds back to the CPU: textures, the draw target and single pixels.
// Each function puts back every binding it changes.

import type { AddressMode, FilterMode, Sampler, Texture, TextureFormat } from '../capture.js';
import { toBase64 } from '../encode.js';
import { renderbuffers, textures } from './state.js';

/** How a texture's internal format reads back and which capture format holds it. */
interface ReadFormat {
  format: TextureFormat;
  /** Channels the capture format keeps out of the four that are read. */
  components: number;
  float: boolean;
}

function readFormat(gl: WebGL2RenderingContext, internal: number): ReadFormat | undefined {
  switch (internal) {
    case gl.RGBA8:
    case gl.RGBA:
    case gl.RGB8:
    case gl.RGB:
    case gl.RG8:
      return { format: 'rgba8unorm', components: 4, float: false };
    case gl.SRGB8_ALPHA8:
    case gl.SRGB8:
      return { format: 'rgba8unorm-srgb', components: 4, float: false };
    case gl.R8:
      return { format: 'r8unorm', components: 1, float: false };
    case gl.R32F:
    case gl.R16F:
      return { format: 'r32float', components: 1, float: true };
    case gl.RG32F:
    case gl.RG16F:
      return { format: 'rg32float', components: 2, float: true };
    case gl.RGBA32F:
    case gl.RGBA16F:
    case gl.RGB32F:
    case gl.RGB16F:
    case gl.R11F_G11F_B10F:
      return { format: 'rgba32float', components: 4, float: true };
    default:
      return undefined;
  }
}

/**
 * Reads every level of a 2D texture through a framebuffer. For a texture it can't read, it
 * returns why instead: another dimension, an integer or compressed format, or a texture made
 * before the hook could see it.
 */
export function readTexture(gl: WebGL2RenderingContext, texture: WebGLTexture): Texture | string {
  const info = textures.get(texture);
  if (!info) return 'it was created before Texelscope was installed';
  if (info.target !== gl.TEXTURE_2D) return 'only 2D textures can be read so far';
  const format = readFormat(gl, info.internalFormat);
  if (!format) return `its internal format 0x${info.internalFormat.toString(16)} can't be read back yet`;
  if (format.float) gl.getExtension('EXT_color_buffer_float');
  const base = info.levels.get(0);
  if (!base) return 'its first level was never uploaded';
  let levels = 0;
  while (info.levels.has(levels)) levels++;

  const savedRead = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  const framebuffer = gl.createFramebuffer();
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, framebuffer);
  const chunks: Uint8Array[] = [];
  try {
    for (let level = 0; level < levels; level++) {
      const [width, height] = info.levels.get(level)!;
      gl.framebufferTexture2D(gl.READ_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, level);
      if (gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
        return `level ${level} can't be attached to a framebuffer to read it`;
      }
      const texels = width * height;
      if (format.float) {
        const all = new Float32Array(texels * 4);
        gl.readPixels(0, 0, width, height, gl.RGBA, gl.FLOAT, all);
        const kept = new Float32Array(texels * format.components);
        for (let t = 0; t < texels; t++) for (let c = 0; c < format.components; c++) kept[t * format.components + c] = all[t * 4 + c]!;
        chunks.push(new Uint8Array(kept.buffer));
      } else {
        const all = new Uint8Array(texels * 4);
        gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, all);
        if (format.components === 4) chunks.push(all);
        else {
          const kept = new Uint8Array(texels * format.components);
          for (let t = 0; t < texels; t++) for (let c = 0; c < format.components; c++) kept[t * format.components + c] = all[t * 4 + c]!;
          chunks.push(kept);
        }
      }
    }
  } finally {
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, savedRead);
    gl.deleteFramebuffer(framebuffer);
  }
  const bytes = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.length;
  }
  // Texel row 0 is the first row uploaded in WebGL as in WebGPU, so rows keep their order.
  return { dimension: '2d', format: format.format, size: [base[0], base[1], 1], mipLevelCount: levels, data: { base64: toBase64(bytes) } };
}

const ADDRESS: Record<number, AddressMode> = {
  0x812f: 'clamp-to-edge',
  0x2901: 'repeat',
  0x8370: 'mirror-repeat',
};

/** How the texture on `unit` is sampled, from its sampler object or its own parameters. */
export function samplerState(gl: WebGL2RenderingContext, target: number, sampler: WebGLSampler | null): Sampler {
  const get = (name: number) => (sampler ? gl.getSamplerParameter(sampler, name) : gl.getTexParameter(target, name)) as number;
  const min = get(gl.TEXTURE_MIN_FILTER);
  const mag = get(gl.TEXTURE_MAG_FILTER);
  const linear = (f: number): FilterMode => (f === gl.LINEAR || f === gl.LINEAR_MIPMAP_NEAREST || f === gl.LINEAR_MIPMAP_LINEAR ? 'linear' : 'nearest');
  const mipLinear = min === gl.NEAREST_MIPMAP_LINEAR || min === gl.LINEAR_MIPMAP_LINEAR;
  return {
    addressModeU: ADDRESS[get(gl.TEXTURE_WRAP_S)] ?? 'repeat',
    addressModeV: ADDRESS[get(gl.TEXTURE_WRAP_T)] ?? 'repeat',
    addressModeW: ADDRESS[get(gl.TEXTURE_WRAP_R)] ?? 'repeat',
    magFilter: linear(mag),
    minFilter: linear(min),
    mipmapFilter: mipLinear ? 'linear' : 'nearest',
  };
}

/** The framebuffer a draw writes to, its size and how its pixels read back. */
export interface DrawTarget {
  framebuffer: WebGLFramebuffer | null;
  width: number;
  height: number;
  float: boolean;
  /** Whether single pixels can be read, which multisampled renderbuffers prevent. */
  readable: boolean;
}

export function drawTarget(gl: WebGL2RenderingContext): DrawTarget {
  const framebuffer = gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  if (!framebuffer) return { framebuffer, width: gl.drawingBufferWidth, height: gl.drawingBufferHeight, float: false, readable: true };
  const attachment = (name: number) => gl.getFramebufferAttachmentParameter(gl.DRAW_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, name);
  const kind = attachment(gl.FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE) as number;
  if (kind === gl.NONE) return { framebuffer, width: 0, height: 0, float: false, readable: false };
  const object = attachment(gl.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME);
  const float = attachment(gl.FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE) === gl.FLOAT;
  if (kind === gl.TEXTURE) {
    const level = attachment(gl.FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL) as number;
    const size = textures.get(object as WebGLTexture)?.levels.get(level);
    return { framebuffer, width: size?.[0] ?? 0, height: size?.[1] ?? 0, float, readable: !!size };
  }
  const info = renderbuffers.get(object as WebGLRenderbuffer);
  const bound = gl.getParameter(gl.RENDERBUFFER_BINDING) as WebGLRenderbuffer | null;
  gl.bindRenderbuffer(gl.RENDERBUFFER, object as WebGLRenderbuffer);
  const samples = gl.getRenderbufferParameter(gl.RENDERBUFFER, gl.RENDERBUFFER_SAMPLES) as number;
  gl.bindRenderbuffer(gl.RENDERBUFFER, bound);
  return { framebuffer, width: info?.width ?? 0, height: info?.height ?? 0, float, readable: !!info && samples === 0 };
}

/** Reads a rectangle of the draw target as RGBA, rows from the bottom as WebGL gives them. */
export function readRect(gl: WebGL2RenderingContext, target: DrawTarget, x: number, glY: number, width: number, height: number): Uint8Array | Float32Array {
  const saved = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, target.framebuffer);
  const savedBuffer = target.framebuffer ? (gl.getParameter(gl.READ_BUFFER) as number) : undefined;
  if (target.framebuffer) gl.readBuffer(gl.COLOR_ATTACHMENT0);
  let out: Uint8Array | Float32Array;
  if (target.float) {
    gl.getExtension('EXT_color_buffer_float');
    out = new Float32Array(width * height * 4);
    gl.readPixels(x, glY, width, height, gl.RGBA, gl.FLOAT, out);
  } else {
    out = new Uint8Array(width * height * 4);
    gl.readPixels(x, glY, width, height, gl.RGBA, gl.UNSIGNED_BYTE, out);
  }
  if (savedBuffer !== undefined) gl.readBuffer(savedBuffer);
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, saved);
  return out;
}

/** Reverses the rows of an image, between WebGL's bottom-first order and the top-first order captures use. */
export function flipRows<T extends Uint8Array | Float32Array>(data: T, width: number, height: number, components: number): T {
  const out = new (data.constructor as { new (n: number): T })(data.length);
  const row = width * components;
  for (let y = 0; y < height; y++) out.set(data.subarray((height - 1 - y) * row, (height - y) * row), y * row);
  return out;
}

/** The whole target as RGBA bytes, rows from the top, for showing and for comparing. */
export function targetBytes(gl: WebGL2RenderingContext, target: DrawTarget): Uint8Array {
  const raw = readRect(gl, target, 0, 0, target.width, target.height);
  const bytes = raw instanceof Uint8Array ? raw : Uint8Array.from(raw, (v) => Math.round(Math.min(1, Math.max(0, v)) * 255));
  return flipRows(bytes, target.width, target.height, 4);
}
