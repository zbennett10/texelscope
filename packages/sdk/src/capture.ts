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
  | { kind: 'fullscreen'; locations: CornerInput[] };

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
