// Replays a capture on the GPU: the captured fragment shader, fed by a generated vertex
// stage, drawn into a texture of the captured size and format.

import type { Capture, Texture } from '@texelscope/sdk';
import type { Engine } from '../engine/client.js';
import type { Description } from '../types.js';

const TEXEL_BYTES: Record<string, number> = {
  r8unorm: 1,
  r32float: 4,
  rg32float: 8,
  rgba8unorm: 4,
  'rgba8unorm-srgb': 4,
  bgra8unorm: 4,
  rgba16float: 8,
  rgba32float: 16,
};
const UNFILTERABLE = new Set(['r32float', 'rg32float', 'rgba32float']);

const BLIT = /* wgsl */ `
@group(0) @binding(0) var image: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
  let c = clamp(textureLoad(image, vec2<i32>(p.xy), 0), vec4<f32>(0.0), vec4<f32>(1.0));
  // Transparent pixels show over a checkerboard.
  let cell = (vec2<i32>(p.xy) / 8) & vec2<i32>(1);
  let checker = select(0.78, 0.9, cell.x == cell.y);
  return vec4<f32>(mix(vec3<f32>(checker), c.rgb, c.a), 1.0);
}`;

export class Replay {
  readonly device: GPUDevice;
  #capture?: Capture;
  #resources = new Map<string, GPUBuffer | GPUTexture | GPUSampler>();
  #target?: GPUTexture;
  #context?: GPUCanvasContext;
  #format: GPUTextureFormat;
  #blit?: GPURenderPipeline;

  static async create(): Promise<Replay> {
    if (!navigator.gpu) throw new Error('this browser has no WebGPU, so only the interpreter runs');
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) throw new Error('no WebGPU adapter is available, so only the interpreter runs');
    return new Replay(await adapter.requestDevice());
  }

  constructor(device: GPUDevice) {
    this.device = device;
    this.#format = navigator.gpu.getPreferredCanvasFormat();
  }

  /** Shows renders on a canvas, which is sized to the target. */
  attach(canvas: HTMLCanvasElement) {
    this.#context = canvas.getContext('webgpu') ?? undefined;
    this.#context?.configure({ device: this.device, format: this.#format, alphaMode: 'opaque' });
  }

  /** Creates the target and every bound resource. */
  async load(capture: Capture, engine: Engine) {
    for (const resource of this.#resources.values()) {
      if (!(resource instanceof GPUSampler)) resource.destroy();
    }
    this.#resources.clear();
    this.#capture = capture;
    this.#target?.destroy();
    this.#target = this.device.createTexture({
      label: 'texelscope target',
      size: [capture.target.width, capture.target.height],
      format: capture.target.format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC | GPUTextureUsage.TEXTURE_BINDING,
    });
    for (const binding of capture.bindings ?? []) {
      const key = `${binding.group}:${binding.binding}`;
      const r = binding.resource;
      if (r.kind === 'texture') {
        this.#resources.set(key, this.#texture(r, await engine.textureBytes(binding.group, binding.binding)));
      } else if (r.kind === 'sampler') {
        this.#resources.set(key, this.device.createSampler({ ...r, label: key }));
      }
    }
    await this.uploadBuffers(engine);
  }

  /** Uploads buffers laid out for the current source, which edits can change. */
  async uploadBuffers(engine: Engine) {
    for (const binding of this.#capture?.bindings ?? []) {
      const r = binding.resource;
      if (r.kind !== 'uniform' && r.kind !== 'storage') continue;
      const key = `${binding.group}:${binding.binding}`;
      let bytes: Uint8Array;
      try {
        bytes = await engine.bufferBytes(binding.group, binding.binding);
      } catch {
        continue;
      }
      (this.#resources.get(key) as GPUBuffer | undefined)?.destroy();
      const usage = r.kind === 'uniform' ? GPUBufferUsage.UNIFORM : GPUBufferUsage.STORAGE;
      const buffer = this.device.createBuffer({ label: key, size: Math.max(16, bytes.byteLength), usage: usage | GPUBufferUsage.COPY_DST });
      this.device.queue.writeBuffer(buffer, 0, bytes);
      this.#resources.set(key, buffer);
    }
  }

  /** Draws the capture with `source` as its shader. Returns an error message on failure. */
  async render(source: string, description: Description): Promise<string | undefined> {
    const capture = this.#capture;
    const target = this.#target;
    if (!capture || !target) return 'no capture is loaded';
    const device = this.device;
    device.pushErrorScope('validation');
    const fragment = device.createShaderModule({ label: 'captured fragment shader', code: source });
    const info = await fragment.getCompilationInfo();
    const errors = info.messages.filter((m) => m.type === 'error');
    if (errors.length > 0) {
      await device.popErrorScope();
      return errors.map((m) => `line ${m.lineNum}: ${m.message}`).join('\n');
    }
    const vertex = device.createShaderModule({ label: 'texelscope vertex stage', code: vertexStage(capture, description) });
    const layouts = this.#layouts(description);
    const pipeline = device.createRenderPipeline({
      label: 'texelscope replay',
      layout: device.createPipelineLayout({ bindGroupLayouts: layouts.map((l) => l.layout) }),
      vertex: { module: vertex, entryPoint: 'texelscope_vs' },
      fragment: { module: fragment, entryPoint: capture.shader.entryPoint, targets: [{ format: capture.target.format }] },
      primitive: { topology: 'triangle-list' },
    });
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{ view: target.createView(), loadOp: 'clear', clearValue: [0, 0, 0, 0], storeOp: 'store' }],
    });
    pass.setPipeline(pipeline);
    layouts.forEach(({ layout, entries }, group) => {
      pass.setBindGroup(group, device.createBindGroup({ layout, entries }));
    });
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
    const error = await device.popErrorScope();
    if (error) return error.message;
    this.present();
    return undefined;
  }

  /** Copies the last render to the attached canvas. */
  present() {
    const target = this.#target;
    const context = this.#context;
    if (!target || !context) return;
    const device = this.device;
    this.#blit ??= device.createRenderPipeline({
      label: 'texelscope blit',
      layout: 'auto',
      vertex: { module: device.createShaderModule({ code: BLIT }), entryPoint: 'vs' },
      fragment: { module: device.createShaderModule({ code: BLIT }), entryPoint: 'fs', targets: [{ format: this.#format }] },
    });
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{ view: context.getCurrentTexture().createView(), loadOp: 'clear', storeOp: 'store' }],
    });
    pass.setPipeline(this.#blit);
    pass.setBindGroup(0, device.createBindGroup({
      layout: this.#blit.getBindGroupLayout(0),
      entries: [{ binding: 0, resource: target.createView() }],
    }));
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
  }

  /** The color the GPU wrote at one pixel, as floats. */
  async readPixel(x: number, y: number): Promise<number[]> {
    const target = this.#target;
    const format = this.#capture?.target.format;
    if (!target || !format) return [];
    const bytes = TEXEL_BYTES[format] ?? 4;
    const buffer = this.device.createBuffer({ size: 256, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    const encoder = this.device.createCommandEncoder();
    encoder.copyTextureToBuffer({ texture: target, origin: { x, y } }, { buffer, bytesPerRow: 256 }, [1, 1]);
    this.device.queue.submit([encoder.finish()]);
    await buffer.mapAsync(GPUMapMode.READ);
    const data = new Uint8Array(buffer.getMappedRange().slice(0, bytes));
    buffer.destroy();
    return decodeTexel(format, data);
  }

  #texture(t: Texture, bytes: Uint8Array): GPUTexture {
    const texture = this.device.createTexture({
      size: t.size,
      dimension: t.dimension,
      format: t.format,
      mipLevelCount: t.mipLevelCount ?? 1,
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    const bpt = TEXEL_BYTES[t.format] ?? 4;
    let offset = 0;
    for (let level = 0; level < (t.mipLevelCount ?? 1); level++) {
      const width = Math.max(1, t.size[0] >> level);
      const height = t.dimension === '1d' ? 1 : Math.max(1, t.size[1] >> level);
      const depth = t.dimension === '3d' ? Math.max(1, t.size[2] >> level) : t.size[2];
      const size = width * height * depth * bpt;
      this.device.queue.writeTexture(
        { texture, mipLevel: level },
        bytes.subarray(offset, offset + size),
        { bytesPerRow: width * bpt, rowsPerImage: height },
        [width, height, depth],
      );
      offset += size;
    }
    return texture;
  }

  /** Bind group layouts and entries for every group the shader declares. */
  #layouts(description: Description): { layout: GPUBindGroupLayout; entries: GPUBindGroupEntry[] }[] {
    const capture = this.#capture!;
    const groups = Math.max(-1, ...description.resources.map((r) => r.group)) + 1;
    return Array.from({ length: groups }, (_, group) => {
      const layoutEntries: GPUBindGroupLayoutEntry[] = [];
      const entries: GPUBindGroupEntry[] = [];
      for (const r of description.resources.filter((r) => r.group === group)) {
        const visibility = GPUShaderStage.FRAGMENT;
        const key = `${group}:${r.binding}`;
        const captured = capture.bindings?.find((b) => b.group === group && b.binding === r.binding)?.resource;
        const k = r.kind;
        if (k.kind === 'uniform' || k.kind === 'storage') {
          const type = k.kind === 'uniform' ? 'uniform' : k.readOnly ? 'read-only-storage' : 'storage';
          layoutEntries.push({ binding: r.binding, visibility, buffer: { type } });
          entries.push({ binding: r.binding, resource: { buffer: this.#buffer(key, k.kind) } });
        } else if (k.kind === 'texture') {
          const format = captured?.kind === 'texture' ? captured.format : 'rgba8unorm';
          const sampleType = k.sampleType === 'float' && UNFILTERABLE.has(format) ? 'unfilterable-float' : k.sampleType;
          layoutEntries.push({
            binding: r.binding,
            visibility,
            texture: { sampleType: sampleType as GPUTextureSampleType, viewDimension: k.viewDimension as GPUTextureViewDimension, multisampled: k.multisampled },
          });
          const texture = this.#resources.get(key) as GPUTexture | undefined;
          entries.push({ binding: r.binding, resource: (texture ?? this.#placeholderTexture(key)).createView({ dimension: k.viewDimension as GPUTextureViewDimension }) });
        } else if (k.kind === 'sampler') {
          const filtering = captured?.kind === 'sampler' && [captured.magFilter, captured.minFilter, captured.mipmapFilter].includes('linear');
          layoutEntries.push({ binding: r.binding, visibility, sampler: { type: k.comparison ? 'comparison' : filtering ? 'filtering' : 'non-filtering' } });
          const sampler = (this.#resources.get(key) as GPUSampler | undefined) ?? this.device.createSampler();
          entries.push({ binding: r.binding, resource: sampler });
        }
      }
      return { layout: this.device.createBindGroupLayout({ entries: layoutEntries }), entries };
    });
  }

  /** The captured buffer, or zeros when the capture left it out. */
  #buffer(key: string, kind: 'uniform' | 'storage'): GPUBuffer {
    const existing = this.#resources.get(key) as GPUBuffer | undefined;
    if (existing) return existing;
    const usage = kind === 'uniform' ? GPUBufferUsage.UNIFORM : GPUBufferUsage.STORAGE;
    const buffer = this.device.createBuffer({ label: `${key} (missing)`, size: 65536, usage });
    this.#resources.set(key, buffer);
    return buffer;
  }

  #placeholderTexture(key: string): GPUTexture {
    const texture = this.device.createTexture({ label: `${key} (missing)`, size: [1, 1], format: 'rgba8unorm', usage: GPUTextureUsage.TEXTURE_BINDING });
    this.#resources.set(key, texture);
    return texture;
  }
}

/** A vertex stage that draws one covering triangle and feeds each location input the
 * values the capture gives for the target's corners. */
export function vertexStage(capture: Capture, description: Description): string {
  const corners = capture.inputs?.kind === 'fullscreen' ? capture.inputs.locations : [];
  const members: string[] = [];
  const assigns: string[] = [];
  for (const input of description.stageInputs) {
    const interpolation = input.interpolation
      ? ` @interpolate(${input.interpolation}${input.sampling ? `, ${input.sampling}` : ''})`
      : '';
    members.push(`  @location(${input.location})${interpolation} v${input.location}: ${input.ty},`);
    const given = corners.find((c) => c.location === input.location);
    if (!given) {
      assigns.push(`  out.v${input.location} = ${input.ty}();`);
      continue;
    }
    const [tl, tr, bl] = given.corners.map((c) => `vec4<f32>(${[0, 1, 2, 3].map((i) => (c[i] ?? 0).toFixed(9)).join(', ')})`);
    const lanes = /vec(\d)/.exec(input.ty)?.[1];
    const pick = lanes ? ['', '.x', '.xy', '.xyz', ''][Number(lanes)] : '.x';
    // Corner values are affine across a full-screen pass, so three of them place the rest.
    assigns.push(`  out.v${input.location} = ${input.ty}((${tl} + s * (${tr} - ${tl}) + t * (${bl} - ${tl}))${pick});`);
  }
  return `struct TexelscopeVertex {
  @builtin(position) position: vec4<f32>,
${members.join('\n')}
}

@vertex
fn texelscope_vs(@builtin(vertex_index) i: u32) -> TexelscopeVertex {
  let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  let s = uv.x;
  let t = uv.y;
  var out: TexelscopeVertex;
  out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
${assigns.join('\n')}
  return out;
}
`;
}

/** Reads one texel in a target format as floats. */
export function decodeTexel(format: string, bytes: Uint8Array): number[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const unorm = (i: number) => bytes[i]! / 255;
  const half = (i: number) => halfToFloat(view.getUint16(i, true));
  switch (format) {
    case 'bgra8unorm':
      return [unorm(2), unorm(1), unorm(0), unorm(3)];
    case 'rgba8unorm':
    case 'rgba8unorm-srgb':
      return [unorm(0), unorm(1), unorm(2), unorm(3)];
    case 'r8unorm':
      return [unorm(0), 0, 0, 1];
    case 'rgba16float':
      return [half(0), half(2), half(4), half(6)];
    case 'r32float':
      return [view.getFloat32(0, true), 0, 0, 1];
    case 'rg32float':
      return [view.getFloat32(0, true), view.getFloat32(4, true), 0, 1];
    default:
      return [0, 4, 8, 12].map((o) => view.getFloat32(o, true));
  }
}

function halfToFloat(bits: number): number {
  const sign = bits & 0x8000 ? -1 : 1;
  const exponent = (bits >> 10) & 0x1f;
  const mantissa = bits & 0x3ff;
  if (exponent === 0) return sign * 2 ** -14 * (mantissa / 1024);
  if (exponent === 0x1f) return mantissa ? NaN : sign * Infinity;
  return sign * 2 ** (exponent - 15) * (1 + mantissa / 1024);
}

/** How a float color lands in a target format, so CPU and GPU colors compare fairly. */
export function quantize(format: string, rgba: number[]): number[] {
  if (format.includes('8unorm')) return rgba.map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255) / 255);
  if (format === 'rgba16float') return rgba.map((c) => halfToFloat(floatToHalf(c)));
  return rgba;
}

function floatToHalf(value: number): number {
  const f = new Float32Array([value]);
  const u = new Uint32Array(f.buffer)[0]!;
  const sign = (u >> 16) & 0x8000;
  const exponent = ((u >> 23) & 0xff) - 127 + 15;
  const mantissa = u & 0x7fffff;
  if (exponent >= 0x1f) return sign | 0x7c00;
  if (exponent <= 0) return sign;
  return sign | (exponent << 10) | Math.round(mantissa / 8192);
}
