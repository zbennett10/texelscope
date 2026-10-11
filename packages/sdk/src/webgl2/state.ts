// What the WebGL2 hook remembers about the app's objects, which WebGL can't report later.

/** A shader's source, as the app gave it. */
export const shaderSources = new WeakMap<WebGLShader, string>();

/** The shaders attached to each program, by stage. */
export const programShaders = new WeakMap<WebGLProgram, { vertex?: WebGLShader; fragment?: WebGLShader }>();

/** A texture's internal format and the size of each level, since WebGL2 can't be asked. */
export interface TextureInfo {
  target: number;
  internalFormat: number;
  levels: Map<number, [number, number, number]>;
}

export const textures = new WeakMap<WebGLTexture, TextureInfo>();

/** A renderbuffer's internal format and size. */
export const renderbuffers = new WeakMap<WebGLRenderbuffer, { internalFormat: number; width: number; height: number }>();

/** One draw call, enough to issue it again. */
export interface DrawCall {
  method: 'drawArrays' | 'drawElements' | 'drawArraysInstanced' | 'drawElementsInstanced' | 'drawRangeElements';
  args: unknown[];
}
