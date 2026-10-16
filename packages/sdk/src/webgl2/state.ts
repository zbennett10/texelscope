// What the WebGL2 hook remembers about the app's objects, which WebGL can't report later.

/** A shader's source, as the app gave it. */
export const shaderSources = new WeakMap<WebGLShader, string>();

/** The shaders attached to each program, by stage. */
export const programShaders = new WeakMap<WebGLProgram, { vertex?: WebGLShader; fragment?: WebGLShader }>();

/** A texture's internal format and the size of each level, since WebGL2 can't be asked. */
export interface TextureInfo {
  target: number;
  internalFormat: number;
  /** The data type of the last upload, which picks the format an unsized one stands for. */
  type?: number;
  levels: Map<number, [number, number, number]>;
}

export const textures = new WeakMap<WebGLTexture, TextureInfo>();

/** How deep the hook is in its own WebGL calls, which the patches leave out of what they note. */
export const own = { depth: 0 };

/** Runs the hook's own WebGL work. */
export function owned<T>(work: () => T): T {
  own.depth++;
  try {
    return work();
  } finally {
    own.depth--;
  }
}

/** Textures the app has attached to a framebuffer, whose contents later frames may change. */
export const renderTargets = new WeakSet<WebGLTexture>();

/** The program and name behind each uniform location the app asked for. */
export const uniformNames = new WeakMap<WebGLUniformLocation, { program: WebGLProgram; name: string }>();

/** A renderbuffer's internal format and size. */
export const renderbuffers = new WeakMap<WebGLRenderbuffer, { internalFormat: number; width: number; height: number }>();

/** One draw call, enough to issue it again. */
export interface DrawCall {
  method: 'drawArrays' | 'drawElements' | 'drawArraysInstanced' | 'drawElementsInstanced' | 'drawRangeElements';
  args: unknown[];
}
