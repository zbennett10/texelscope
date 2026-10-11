// Mirror of the Rust types in crates/texelscope-capture. docs/capture-format.md describes the format.

/** The capture format version this package writes. */
export const FORMAT_VERSION = 0;

/** A recorded fragment workload that Texelscope can replay. */
export interface Capture {
  version: number;
  label?: string;
  /** Frame index in the app the capture came from. */
  frame?: number;
  shader: Shader;
  target: Target;
  /** Defaults to `{ kind: 'builtins' }`. */
  inputs?: FragmentInputs;
  bindings?: Binding[];
  /** What the app's GPU drew, for comparing with the interpreter and for showing. */
  reference?: Reference;
}

/** Pixels the app's GPU drew into the target. */
export interface Reference {
  /** Where the pixels sit in the target. */
  rect: Rect;
  /** RGBA, one byte each, rows from the top, as base64. */
  rgba8: string;
}

/** A rectangle of pixels, counted from the top left. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Shader {
  language: 'wgsl' | 'glsl';
  /** Inline source, or a path relative to the capture file. */
  source: { inline: string } | { path: string };
  /** The fragment entry point to replay. */
  entryPoint: string;
}

/** The color target the draw wrote to. */
export interface Target {
  width: number;
  height: number;
  format: TextureFormat;
  /** Whether the API counts window y up from the bottom, as WebGL does. Pixels are still numbered from the top. */
  yUp?: boolean;
}

export type TextureFormat =
  | 'r8unorm'
  | 'r32float'
  | 'rg32float'
  | 'rgba8unorm'
  | 'rgba8unorm-srgb'
  | 'bgra8unorm'
  | 'rgba16float'
  | 'rgba32float';

export type FragmentInputs =
  | { kind: 'builtins' }
  | { kind: 'fullscreen'; locations: CornerInput[] }
  | {
      kind: 'window';
      rect: Rect;
      /** The fragment position at each pixel, four floats, rows from the top, as base64 of f32s. */
      fragCoord: string;
      varyings: WindowInput[];
    };

/** One input over a window, NaN where the draw didn't cover a pixel. */
export interface WindowInput {
  name: string;
  /** Floats per pixel. */
  components: number;
  /** Rows from the top, as base64 of little-endian f32s. */
  data: string;
}

/** A user input interpolated across the target from its four corner values. */
export interface CornerInput {
  location: number;
  /** Values at the top-left, top-right, bottom-left and bottom-right corners. */
  corners: [number[], number[], number[], number[]];
}

export interface Binding {
  group: number;
  binding: number;
  label?: string;
  /** The GLSL uniform, block or sampler this binds. WebGL binds by name, so `group` and `binding` are then ignored. */
  name?: string;
  resource: Resource;
}

export type Resource =
  | { kind: 'uniform'; data: BufferData }
  | { kind: 'storage'; data: BufferData }
  | ({ kind: 'texture' } & Texture)
  | ({ kind: 'sampler' } & Sampler);

/** Member values by name, or the raw bytes the shader sees. */
export type BufferData =
  | { values: Record<string, unknown> }
  | { base64: string }
  | { path: string };

export interface Texture {
  dimension: '1d' | '2d' | '3d';
  format: TextureFormat;
  /** Width, height and depth or array layer count. */
  size: [number, number, number];
  /** Defaults to 1. */
  mipLevelCount?: number;
  data: TextureData;
}

/** Inline texel components, all mip levels in order with rows from the top, or raw bytes. */
export type TextureData =
  | { inline: number[] }
  | { base64: string }
  | { path: string };

/** Fields and defaults follow GPUSamplerDescriptor. */
export interface Sampler {
  addressModeU?: AddressMode;
  addressModeV?: AddressMode;
  addressModeW?: AddressMode;
  magFilter?: FilterMode;
  minFilter?: FilterMode;
  mipmapFilter?: FilterMode;
}

export type AddressMode = 'clamp-to-edge' | 'repeat' | 'mirror-repeat';

export type FilterMode = 'nearest' | 'linear';
