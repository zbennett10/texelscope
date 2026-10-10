import { afterEach, describe, expect, it } from 'vitest';
import type { Capture } from '../src/capture.js';
import { fromBase64 } from '../src/encode.js';
import { installFakeWebGPU } from './fake-webgpu.js';

const SHADER = `
struct Params { gain: f32, bias: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var data: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(1) @binding(0) var<storage, read> extra: array<f32>;
@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }
@fragment fn shade(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> { return vec4<f32>(params.gain); }
`;

const device = installFakeWebGPU();
const { installWebGPU } = await import('../src/webgpu/index.js');

let uninstall: (() => void) | undefined;
afterEach(() => uninstall?.());

/** Draws once with a uniform at a dynamic offset, a 3x2 texture with two mips, a sampler
 * and a storage buffer under an automatic layout. */
function draw(app: GPUDevice) {
  const module = app.createShaderModule({ code: SHADER });
  const layout = app.createBindGroupLayout({
    entries: [
      { binding: 0, visibility: 2, buffer: { type: 'uniform', hasDynamicOffset: true } },
      { binding: 1, visibility: 2, texture: { sampleType: 'unfilterable-float' } },
      { binding: 2, visibility: 2, sampler: {} },
    ],
  });
  const pipeline = app.createRenderPipeline({
    label: 'field',
    layout: 'auto',
    vertex: { module, entryPoint: 'vs' },
    fragment: { module, targets: [{ format: 'bgra8unorm' }] },
  });

  const uniforms = app.createBuffer({ size: 512, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  app.queue.writeBuffer(uniforms, 256, new Float32Array([2.5, -1]));
  const texture = app.createTexture({
    size: [3, 2, 1],
    mipLevelCount: 2,
    format: 'r32float',
    usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
  });
  app.queue.writeTexture({ texture, mipLevel: 0 }, new Float32Array([1, 2, 3, 4, 5, 6]), {}, [3, 2]);
  app.queue.writeTexture({ texture, mipLevel: 1 }, new Float32Array([9]), {}, [1, 1]);
  const sampler = app.createSampler({ magFilter: 'linear', addressModeU: 'repeat' });
  const extra = app.createBuffer({ size: 8, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
  app.queue.writeBuffer(extra, 0, new Float32Array([7, 8]));

  const group0 = app.createBindGroup({
    layout,
    entries: [
      { binding: 0, resource: { buffer: uniforms, size: 8 } },
      { binding: 1, resource: texture.createView() },
      { binding: 2, resource: sampler },
    ],
  });
  const group1 = app.createBindGroup({
    layout: pipeline.getBindGroupLayout?.(1) ?? app.createBindGroupLayout({ entries: [] }),
    entries: [{ binding: 0, resource: { buffer: extra } }],
  });

  const target = app.createTexture({ size: [64, 32], format: 'bgra8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT });
  const encoder = app.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{ view: target.createView(), loadOp: 'clear', storeOp: 'store' }],
  });
  pass.setPipeline(pipeline);
  pass.setBindGroup(0, group0, new Uint32Array([0, 256]), 1, 1);
  pass.setBindGroup(1, group1);
  pass.draw(3);
  pass.end();
  app.queue.submit([encoder.finish()]);
  // A write after the submit must not show up in the capture.
  app.queue.writeBuffer(uniforms, 256, new Float32Array([99, 99]));
}

describe('installWebGPU', () => {
  it('captures a draw with every binding read back', async () => {
    const session = installWebGPU();
    uninstall = session.uninstall;
    const pending = session.captureNextDraw();
    draw(new (globalThis as unknown as { GPUDevice: new () => GPUDevice }).GPUDevice());
    const capture: Capture = await pending;

    expect(capture.label).toBe('field');
    expect(capture.shader).toMatchObject({ language: 'wgsl', entryPoint: 'shade' });
    expect(capture.target).toEqual({ width: 64, height: 32, format: 'bgra8unorm' });

    const [uniform, texture, sampler, storage] = capture.bindings!;
    expect(uniform).toMatchObject({ group: 0, binding: 0, resource: { kind: 'uniform' } });
    const uniformBytes = fromBase64((uniform!.resource as { data: { base64: string } }).data.base64);
    expect(Array.from(new Float32Array(uniformBytes.buffer))).toEqual([2.5, -1]);

    expect(texture!.resource).toMatchObject({ kind: 'texture', format: 'r32float', size: [3, 2, 1], mipLevelCount: 2 });
    const texels = fromBase64((texture!.resource as { data: { base64: string } }).data.base64);
    expect(Array.from(new Float32Array(texels.buffer))).toEqual([1, 2, 3, 4, 5, 6, 9]);

    expect(sampler!.resource).toEqual({ kind: 'sampler', magFilter: 'linear', addressModeU: 'repeat' });

    // The automatic layout says nothing, so the shader's declaration decides the kind.
    expect(storage).toMatchObject({ group: 1, binding: 0, resource: { kind: 'storage' } });
  });

  it('puts WebGPU back when uninstalled', () => {
    const original = device.constructor.prototype.createBuffer;
    const session = installWebGPU();
    expect(device.constructor.prototype.createBuffer).not.toBe(original);
    session.uninstall();
    uninstall = undefined;
    expect(device.constructor.prototype.createBuffer).toBe(original);
  });
});
