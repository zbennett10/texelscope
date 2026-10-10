// A small in-memory WebGPU, enough to drive the hook through a draw and a readback.

type Command =
  | { kind: 'buffer'; source: FakeBuffer; sourceOffset: number; target: FakeBuffer; targetOffset: number; size: number }
  | { kind: 'texture'; source: GPUImageCopyTexture; target: GPUImageCopyBuffer; size: GPUExtent3DDict };

class FakeBuffer {
  bytes: Uint8Array;
  constructor(readonly size: number, readonly usage: number) {
    this.bytes = new Uint8Array(size);
  }
  mapAsync() {
    return Promise.resolve();
  }
  getMappedRange(offset = 0, size = this.size - offset) {
    return this.bytes.slice(offset, offset + size).buffer;
  }
  unmap() {}
  destroy() {}
}

class FakeTexture {
  readonly levels: Uint8Array[];
  constructor(readonly d: GPUTextureDescriptor) {
    const [w, h, layers] = extent(d.size);
    this.levels = Array.from({ length: d.mipLevelCount ?? 1 }, (_, level) =>
      new Uint8Array(Math.max(1, w >> level) * Math.max(1, h >> level) * layers * texelBytes(d.format)),
    );
  }
  get width() { return extent(this.d.size)[0]; }
  get height() { return extent(this.d.size)[1]; }
  get depthOrArrayLayers() { return extent(this.d.size)[2]; }
  get mipLevelCount() { return this.d.mipLevelCount ?? 1; }
  get sampleCount() { return this.d.sampleCount ?? 1; }
  get dimension() { return this.d.dimension ?? '2d'; }
  get format() { return this.d.format; }
  get usage() { return this.d.usage; }
  createView(_d?: GPUTextureViewDescriptor) {
    return new FakeTextureView();
  }
}

class FakeTextureView {}
class FakeSampler {}
class FakeShaderModule {}
class FakeRenderPipeline {}
class FakeBindGroupLayout {}
class FakeBindGroup {}
class FakeCommandBuffer {
  constructor(readonly commands: Command[]) {}
}

class FakeRenderPassEncoder {
  setPipeline() {}
  setBindGroup() {}
  draw() {}
  drawIndexed() {}
  drawIndirect() {}
  drawIndexedIndirect() {}
  end() {}
}

class FakeCommandEncoder {
  commands: Command[] = [];
  beginRenderPass(_d: GPURenderPassDescriptor) {
    return new FakeRenderPassEncoder();
  }
  copyBufferToBuffer(source: FakeBuffer, sourceOffset: number, target: FakeBuffer, targetOffset: number, size: number) {
    this.commands.push({ kind: 'buffer', source, sourceOffset, target, targetOffset, size });
  }
  copyTextureToBuffer(source: GPUImageCopyTexture, target: GPUImageCopyBuffer, size: GPUExtent3DDict) {
    this.commands.push({ kind: 'texture', source, target, size });
  }
  finish() {
    return new FakeCommandBuffer(this.commands);
  }
}

class FakeQueue {
  writeBuffer(buffer: FakeBuffer, offset: number, data: ArrayBufferView) {
    buffer.bytes.set(new Uint8Array(data.buffer, data.byteOffset, data.byteLength), offset);
  }
  writeTexture(destination: GPUImageCopyTexture, data: ArrayBufferView) {
    const texture = destination.texture as unknown as FakeTexture;
    texture.levels[destination.mipLevel ?? 0]!.set(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  }
  submit(buffers: FakeCommandBuffer[]) {
    for (const command of buffers.flatMap((b) => b.commands)) run(command);
  }
}

class FakeDevice {
  readonly queue = new FakeQueue();
  createShaderModule(_d: GPUShaderModuleDescriptor) { return new FakeShaderModule(); }
  createRenderPipeline(_d: GPURenderPipelineDescriptor) { return new FakeRenderPipeline(); }
  createRenderPipelineAsync(_d: GPURenderPipelineDescriptor) { return Promise.resolve(new FakeRenderPipeline()); }
  createBindGroupLayout(_d: GPUBindGroupLayoutDescriptor) { return new FakeBindGroupLayout(); }
  createBindGroup(_d: GPUBindGroupDescriptor) { return new FakeBindGroup(); }
  createSampler(_d?: GPUSamplerDescriptor) { return new FakeSampler(); }
  createCommandEncoder() { return new FakeCommandEncoder(); }
  createBuffer(d: GPUBufferDescriptor) { return new FakeBuffer(d.size, d.usage); }
  createTexture(d: GPUTextureDescriptor) { return new FakeTexture(d); }
  pushErrorScope() {}
  popErrorScope() { return Promise.resolve(null); }
}

function run(command: Command) {
  if (command.kind === 'buffer') {
    const bytes = command.source.bytes.subarray(command.sourceOffset, command.sourceOffset + command.size);
    command.target.bytes.set(bytes, command.targetOffset);
    return;
  }
  const texture = command.source.texture as unknown as FakeTexture;
  const level = command.source.mipLevel ?? 0;
  const bpt = texelBytes(texture.format);
  const { width, height = 1, depthOrArrayLayers = 1 } = command.size;
  const target = command.target.buffer as unknown as FakeBuffer;
  const rowBytes = width * bpt;
  for (let row = 0; row < height * depthOrArrayLayers; row++) {
    const from = texture.levels[level]!.subarray(row * rowBytes, (row + 1) * rowBytes);
    target.bytes.set(from, row * command.target.bytesPerRow!);
  }
}

function extent(size: GPUExtent3D): [number, number, number] {
  if (Symbol.iterator in Object(size)) {
    const [w, h = 1, d = 1] = [...(size as Iterable<number>)];
    return [w!, h, d];
  }
  const dict = size as GPUExtent3DDict;
  return [dict.width, dict.height ?? 1, dict.depthOrArrayLayers ?? 1];
}

function texelBytes(format: GPUTextureFormat): number {
  return ({ r32float: 4, rg32float: 8, rgba8unorm: 4, bgra8unorm: 4, rgba32float: 16 } as Record<string, number>)[format] ?? 4;
}

/** Puts the fake classes and constants on globalThis, the way a browser exposes WebGPU. */
export function installFakeWebGPU() {
  Object.assign(globalThis, {
    GPUDevice: FakeDevice,
    GPUQueue: FakeQueue,
    GPUBuffer: FakeBuffer,
    GPUTexture: FakeTexture,
    GPUTextureView: FakeTextureView,
    GPUSampler: FakeSampler,
    GPUCommandEncoder: FakeCommandEncoder,
    GPURenderPassEncoder: FakeRenderPassEncoder,
    GPUBufferUsage: { MAP_READ: 1, MAP_WRITE: 2, COPY_SRC: 4, COPY_DST: 8, INDEX: 16, VERTEX: 32, UNIFORM: 64, STORAGE: 128 },
    GPUTextureUsage: { COPY_SRC: 1, COPY_DST: 2, TEXTURE_BINDING: 4, STORAGE_BINDING: 8, RENDER_ATTACHMENT: 16 },
    GPUMapMode: { READ: 1, WRITE: 2 },
  });
  return new FakeDevice() as unknown as GPUDevice;
}
