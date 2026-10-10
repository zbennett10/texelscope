/** What the hook remembers about objects the app creates, keyed weakly so nothing leaks. */
export const shaders = new WeakMap<GPUShaderModule, { code: string; label?: string }>();
export const pipelines = new WeakMap<GPURenderPipeline, GPURenderPipelineDescriptor>();
export const layouts = new WeakMap<GPUBindGroupLayout, GPUBindGroupLayoutEntry[]>();
export const bindGroups = new WeakMap<GPUBindGroup, { layout: GPUBindGroupLayout; entries: GPUBindGroupEntry[] }>();
export const views = new WeakMap<GPUTextureView, { texture: GPUTexture; descriptor?: GPUTextureViewDescriptor }>();
export const samplers = new WeakMap<GPUSampler, GPUSamplerDescriptor>();
export const encoderDevice = new WeakMap<GPUCommandEncoder, GPUDevice>();

/** A render pass's current pipeline, bind groups and first color attachment. */
export interface PassState {
  encoder: GPUCommandEncoder;
  target?: GPUTextureView;
  pipeline?: GPURenderPipeline;
  bindGroups: { group: GPUBindGroup; offsets: number[] }[];
}
export const passes = new WeakMap<GPURenderPassEncoder, PassState>();

/** Everything a draw used, taken when the draw is encoded. */
export interface Snapshot {
  device: GPUDevice;
  label?: string;
  code: string;
  entryPoint?: string;
  target?: GPUTextureView;
  bindGroups: { index: number; group: GPUBindGroup; offsets: number[] }[];
  frame: number;
}

/** A draw waiting for its command buffer to be submitted, and who wants it. */
export interface PendingCapture {
  snapshot: Snapshot;
  resolve: (snapshot: Snapshot) => void;
}
export const pendingByEncoder = new WeakMap<GPUCommandEncoder, PendingCapture[]>();
export const pendingByBuffer = new WeakMap<GPUCommandBuffer, PendingCapture[]>();

/** Counts frames by the canvas textures the app draws into. */
export const frames = { count: 0, last: new WeakMap<GPUCanvasContext, GPUTexture>() };
