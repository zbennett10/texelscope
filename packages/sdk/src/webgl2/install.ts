import { patcher } from '../webgpu/patch.js';
import { type DrawCall, own, programShaders, renderTargets, renderbuffers, shaderSources, type TextureInfo, textures, uniformNames } from './state.js';

export interface GLHooks {
  /** Called just before each draw call. */
  beforeDraw(gl: WebGL2RenderingContext, call: DrawCall): void;
  /** Called just after each draw call, while its state is still bound. */
  afterDraw(gl: WebGL2RenderingContext, call: DrawCall): void;
}

/** The draw methods as WebGL defined them, for issuing a draw again without the hooks. */
export const originalDraws: Partial<Record<DrawCall['method'], (...args: unknown[]) => void>> = {};

const DRAWS: DrawCall['method'][] = ['drawArrays', 'drawElements', 'drawArraysInstanced', 'drawElementsInstanced', 'drawRangeElements'];

/** The binding to ask about for each texture target an upload names. */
function bindingFor(gl: WebGL2RenderingContext, target: number): number {
  if (target === gl.TEXTURE_3D) return gl.TEXTURE_BINDING_3D;
  if (target === gl.TEXTURE_2D_ARRAY) return gl.TEXTURE_BINDING_2D_ARRAY;
  if (target >= gl.TEXTURE_CUBE_MAP_POSITIVE_X && target <= gl.TEXTURE_CUBE_MAP_NEGATIVE_Z) return gl.TEXTURE_BINDING_CUBE_MAP;
  if (target === gl.TEXTURE_CUBE_MAP) return gl.TEXTURE_BINDING_CUBE_MAP;
  return gl.TEXTURE_BINDING_2D;
}

/** Notes a texture level's format and size, against the texture bound for the upload. */
function noteLevel(gl: WebGL2RenderingContext, target: number, level: number, internalFormat: number, size: [number, number, number], type?: number) {
  const texture = gl.getParameter(bindingFor(gl, target)) as WebGLTexture | null;
  if (!texture) return;
  const info: TextureInfo = textures.get(texture) ?? { target, internalFormat, levels: new Map() };
  info.internalFormat = internalFormat;
  info.target = target;
  info.type = type;
  info.levels.set(level, size);
  textures.set(texture, info);
}

/** The size of an image an upload takes from, such as an image, canvas or video. */
function sourceSize(source: unknown): [number, number] {
  const s = source as { videoWidth?: number; videoHeight?: number; displayWidth?: number; displayHeight?: number; width?: number; height?: number };
  return [s.videoWidth || s.displayWidth || s.width || 0, s.videoHeight || s.displayHeight || s.height || 0];
}

/** Patches WebGL2 so draws can be captured. Returns the undo. */
export function installPatches(hooks: GLHooks): () => void {
  const { patch, restore } = patcher();
  const proto = WebGL2RenderingContext.prototype;

  patch(proto, 'shaderSource', (original) => function (this: WebGL2RenderingContext, shader: WebGLShader, source: string) {
    shaderSources.set(shader, source);
    return original.call(this, shader, source);
  });

  patch(proto, 'attachShader', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram, shader: WebGLShader) {
    const stages = programShaders.get(program) ?? {};
    if (this.getShaderParameter(shader, this.SHADER_TYPE) === this.FRAGMENT_SHADER) stages.fragment = shader;
    else stages.vertex = shader;
    programShaders.set(program, stages);
    return original.call(this, program, shader);
  });

  patch(proto, 'texImage2D', (original) => function (this: WebGL2RenderingContext, ...args: unknown[]) {
    const [target, level, internalFormat] = args as number[];
    // Nine or more arguments give a width and height, six take them from an image.
    const sized = args.length >= 8;
    const size: [number, number] = sized ? [args[3] as number, args[4] as number] : sourceSize(args[5]);
    noteLevel(this, target!, level!, internalFormat!, [size[0], size[1], 1], (sized ? args[7] : args[4]) as number);
    return (original as (...a: unknown[]) => void).apply(this, args);
  } as typeof proto.texImage2D);

  patch(proto, 'texStorage2D', (original) => function (this: WebGL2RenderingContext, target: number, levels: number, internalFormat: number, width: number, height: number) {
    for (let level = 0; level < levels; level++) {
      noteLevel(this, target, level, internalFormat, [Math.max(1, width >> level), Math.max(1, height >> level), 1]);
    }
    return original.call(this, target, levels, internalFormat, width, height);
  });

  patch(proto, 'texImage3D', (original) => function (this: WebGL2RenderingContext, ...args: unknown[]) {
    const [target, level, internalFormat, width, height, depth] = args as number[];
    noteLevel(this, target!, level!, internalFormat!, [width!, height!, depth!], args[8] as number);
    return (original as (...a: unknown[]) => void).apply(this, args);
  } as typeof proto.texImage3D);

  patch(proto, 'texStorage3D', (original) => function (this: WebGL2RenderingContext, target: number, levels: number, internalFormat: number, width: number, height: number, depth: number) {
    for (let level = 0; level < levels; level++) {
      noteLevel(this, target, level, internalFormat, [Math.max(1, width >> level), Math.max(1, height >> level), depth]);
    }
    return original.call(this, target, levels, internalFormat, width, height, depth);
  });

  patch(proto, 'framebufferTexture2D', (original) => function (this: WebGL2RenderingContext, target: number, attachment: number, textarget: number, texture: WebGLTexture | null, level: number) {
    if (texture && own.depth === 0) renderTargets.add(texture);
    return original.call(this, target, attachment, textarget, texture, level);
  });

  patch(proto, 'framebufferTextureLayer', (original) => function (this: WebGL2RenderingContext, target: number, attachment: number, texture: WebGLTexture | null, level: number, layer: number) {
    if (texture && own.depth === 0) renderTargets.add(texture);
    return original.call(this, target, attachment, texture, level, layer);
  });

  patch(proto, 'getUniformLocation', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram, name: string) {
    const location = original.call(this, program, name);
    if (location) uniformNames.set(location, { program, name });
    return location;
  });

  patch(proto, 'renderbufferStorage', (original) => function (this: WebGL2RenderingContext, target: number, internalFormat: number, width: number, height: number) {
    const renderbuffer = this.getParameter(this.RENDERBUFFER_BINDING) as WebGLRenderbuffer | null;
    if (renderbuffer) renderbuffers.set(renderbuffer, { internalFormat, width, height });
    return original.call(this, target, internalFormat, width, height);
  });

  patch(proto, 'renderbufferStorageMultisample', (original) => function (this: WebGL2RenderingContext, target: number, samples: number, internalFormat: number, width: number, height: number) {
    const renderbuffer = this.getParameter(this.RENDERBUFFER_BINDING) as WebGLRenderbuffer | null;
    if (renderbuffer) renderbuffers.set(renderbuffer, { internalFormat, width, height });
    return original.call(this, target, samples, internalFormat, width, height);
  });

  for (const method of DRAWS) {
    patch(proto, method, (original) => {
      originalDraws[method] = original as (...args: unknown[]) => void;
      return function (this: WebGL2RenderingContext, ...args: unknown[]) {
        const call: DrawCall = { method, args };
        hooks.beforeDraw(this, call);
        (original as (...a: unknown[]) => void).apply(this, args);
        hooks.afterDraw(this, call);
      } as never;
    });
  }

  return restore;
}
