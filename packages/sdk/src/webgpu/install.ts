import { patch } from './patch.js';
import {
  bindGroups, encoderDevice, frames, layouts, passes, pendingByBuffer, pendingByEncoder, pipelines, samplers,
  shaders, views,
} from './state.js';

export interface Hooks {
  /** Called before each draw, with the pass it is encoded in. */
  onDraw(pass: GPURenderPassEncoder): void;
  /** Called after the app's command buffers are submitted. */
  onSubmit(queue: GPUQueue, buffers: GPUCommandBuffer[]): void;
}

const DEPTH_OR_STENCIL = /^(depth|stencil)/;

/** Patches WebGPU's prototypes to record what draws use. Undo with restoreAll. */
export function installPatches(hooks: Hooks) {
  patch(GPUDevice.prototype, 'createShaderModule', (original) => function (this: GPUDevice, d: GPUShaderModuleDescriptor) {
    const shaderModule = original.call(this, d);
    shaders.set(shaderModule, d);
    return shaderModule;
  });

  patch(GPUDevice.prototype, 'createRenderPipeline', (original) => function (this: GPUDevice, d: GPURenderPipelineDescriptor) {
    const renderPipeline = original.call(this, d);
    pipelines.set(renderPipeline, d);
    return renderPipeline;
  });

  patch(GPUDevice.prototype, 'createRenderPipelineAsync', (original) => function (this: GPUDevice, d: GPURenderPipelineDescriptor) {
    // The pipeline only exists once the promise settles, so record it then.
    return original.call(this, d).then((renderPipeline) => {
      pipelines.set(renderPipeline, d);
      return renderPipeline;
    });
  });

  patch(GPUDevice.prototype, 'createBindGroupLayout', (original) => function (this: GPUDevice, d: GPUBindGroupLayoutDescriptor) {
    const bindGroupLayout = original.call(this, d);
    layouts.set(bindGroupLayout, [...d.entries]);
    return bindGroupLayout;
  });

  patch(GPUDevice.prototype, 'createBindGroup', (original) => function (this: GPUDevice, d: GPUBindGroupDescriptor) {
    const bindGroup = original.call(this, d);
    bindGroups.set(bindGroup, { layout: d.layout, entries: [...d.entries] });
    return bindGroup;
  });

  patch(GPUDevice.prototype, 'createSampler', (original) => function (this: GPUDevice, d?: GPUSamplerDescriptor) {
    const sampler = original.call(this, d);
    samplers.set(sampler, d ?? {});
    return sampler;
  });

  patch(GPUDevice.prototype, 'createCommandEncoder', (original) => function (this: GPUDevice, d?: GPUCommandEncoderDescriptor) {
    const commandEncoder = original.call(this, d);
    encoderDevice.set(commandEncoder, this);
    return commandEncoder;
  });

  patch(GPUDevice.prototype, 'createBuffer', (original) => function (this: GPUDevice, d: GPUBufferDescriptor) {
    // MAP_READ buffers can't take COPY_SRC.
    const usage = d.usage & GPUBufferUsage.MAP_READ ? d.usage : d.usage | GPUBufferUsage.COPY_SRC;
    return original.call(this, { ...d, usage });
  });

  patch(GPUDevice.prototype, 'createTexture', (original) => function (this: GPUDevice, d: GPUTextureDescriptor) {
    // Multisampled and depth or stencil textures are left alone, since they can't be read back as texels.
    const copyable = (d.sampleCount ?? 1) === 1 && !DEPTH_OR_STENCIL.test(d.format);
    const usage = copyable ? d.usage | GPUTextureUsage.COPY_SRC : d.usage;
    return original.call(this, { ...d, usage });
  });

  patch(GPUTexture.prototype, 'createView', (original) => function (this: GPUTexture, d?: GPUTextureViewDescriptor) {
    const view = original.call(this, d);
    views.set(view, { texture: this, descriptor: d });
    return view;
  });

  if (typeof GPUCanvasContext !== 'undefined') {
    patch(GPUCanvasContext.prototype, 'getCurrentTexture', (original) => function (this: GPUCanvasContext) {
      const texture = original.call(this);
      if (frames.last.get(this) !== texture) {
        frames.last.set(this, texture);
        frames.count += 1;
      }
      return texture;
    });
  }

  patch(GPUCommandEncoder.prototype, 'beginRenderPass', (original) => function (this: GPUCommandEncoder, d: GPURenderPassDescriptor) {
    const pass = original.call(this, d);
    const first = [...d.colorAttachments][0];
    passes.set(pass, { encoder: this, target: first?.view as GPUTextureView | undefined, bindGroups: [] });
    return pass;
  });

  patch(GPURenderPassEncoder.prototype, 'setPipeline', (original) => function (this: GPURenderPassEncoder, renderPipeline: GPURenderPipeline) {
    const state = passes.get(this);
    if (state) state.pipeline = renderPipeline;
    return original.call(this, renderPipeline);
  });

  patch(GPURenderPassEncoder.prototype, 'setBindGroup', (original) => function (this: GPURenderPassEncoder, ...args: unknown[]) {
    const [index, group, offsets, start, length] = args as [
      number, GPUBindGroup | null, Iterable<number> | Uint32Array | undefined, number | undefined, number | undefined,
    ];
    const state = passes.get(this);
    if (state && group) state.bindGroups[index] = { group, offsets: dynamicOffsets(offsets, start, length) };
    else if (state) delete state.bindGroups[index];
    return (original as (...a: unknown[]) => undefined).apply(this, args);
  } as GPURenderPassEncoder['setBindGroup']);

  for (const method of ['draw', 'drawIndexed', 'drawIndirect', 'drawIndexedIndirect'] as const) {
    patch(GPURenderPassEncoder.prototype, method, (original) => function (this: GPURenderPassEncoder, ...args: unknown[]) {
      hooks.onDraw(this);
      return (original as (...a: unknown[]) => undefined).apply(this, args);
    } as GPURenderPassEncoder[typeof method]);
  }

  patch(GPUCommandEncoder.prototype, 'finish', (original) => function (this: GPUCommandEncoder, d?: GPUCommandBufferDescriptor) {
    const buffer = original.call(this, d);
    const pending = pendingByEncoder.get(this);
    if (pending) {
      pendingByBuffer.set(buffer, pending);
      pendingByEncoder.delete(this);
    }
    return buffer;
  });

  patch(GPUQueue.prototype, 'submit', (original) => function (this: GPUQueue, buffers: Iterable<GPUCommandBuffer>) {
    const list = [...buffers];
    const result = original.call(this, list);
    hooks.onSubmit(this, list);
    return result;
  });
}

function dynamicOffsets(offsets?: Iterable<number> | Uint32Array, start?: number, length?: number): number[] {
  if (!offsets) return [];
  if (offsets instanceof Uint32Array && start !== undefined) {
    return Array.from(offsets.subarray(start, start + (length ?? 0)));
  }
  return Array.from(offsets);
}
