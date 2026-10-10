import type { Binding, Capture, FragmentInputs, Sampler, Texture, TextureFormat } from '../capture.js';
import { FORMAT_VERSION } from '../capture.js';
import { toBase64 } from '../encode.js';
import { fragmentEntry, resourceKinds } from '../wgsl.js';
import { bindGroups, layouts, samplers, views, type Snapshot } from './state.js';

/** Bytes per texel of every format a capture can hold. */
const TEXEL_BYTES: Record<TextureFormat, number> = {
  r8unorm: 1,
  r32float: 4,
  rg32float: 8,
  rgba8unorm: 4,
  'rgba8unorm-srgb': 4,
  bgra8unorm: 4,
  rgba16float: 8,
  rgba32float: 16,
};

const align = (n: number, to: number) => Math.ceil(n / to) * to;

/**
 * Copies every resource a draw used and builds its capture. The copies are encoded and
 * submitted before this returns its promise, so later writes by the app are not seen.
 */
export function readCapture(snapshot: Snapshot, inputs: FragmentInputs): Promise<Capture> {
  const { device } = snapshot;
  device.pushErrorScope('validation');
  const encoder = device.createCommandEncoder({ label: 'texelscope readback' });
  const kinds = resourceKinds(snapshot.code);
  // Reads map their staging buffers, which may only happen after the copies are submitted.
  const jobs: (() => Promise<Binding>)[] = [];
  const skipped: string[] = [];

  for (const { index: group, group: bindGroup, offsets } of snapshot.bindGroups) {
    const info = bindGroups.get(bindGroup);
    if (!info) continue;
    const layoutEntries = layouts.get(info.layout) ?? [];
    const dynamic = dynamicOffsetsByBinding(layoutEntries, offsets);
    for (const entry of info.entries) {
      const at = { group, binding: entry.binding };
      const resource = entry.resource;
      if (resource instanceof GPUSampler) {
        const sampler = samplerOf(resource);
        jobs.push(() => Promise.resolve({ ...at, resource: { kind: 'sampler', ...sampler } }));
      } else if (resource instanceof GPUTextureView) {
        const read = readTexture(device, encoder, resource);
        if (typeof read === 'string') skipped.push(`@group(${group}) @binding(${entry.binding}): ${read}`);
        else jobs.push(() => read().then((texture) => ({ ...at, resource: { kind: 'texture', ...texture } })));
      } else if (resource instanceof GPUBuffer || (typeof resource === 'object' && 'buffer' in resource)) {
        const binding = resource instanceof GPUBuffer ? { buffer: resource } : (resource as GPUBufferBinding);
        const layoutType = layoutEntries.find((e) => e.binding === entry.binding)?.buffer?.type;
        const kind = layoutType
          ? (layoutType === 'uniform' ? 'uniform' : 'storage')
          : (kinds.get(`${group}:${entry.binding}`) ?? 'uniform');
        const read = readBuffer(device, encoder, binding, dynamic.get(entry.binding) ?? 0);
        jobs.push(() =>
          read().then((bytes) => ({
            ...at,
            resource: { kind, data: { base64: toBase64(bytes) } },
          })),
        );
      } else {
        skipped.push(`@group(${group}) @binding(${entry.binding}) holds a resource Texelscope can't capture yet`);
      }
    }
  }
  device.queue.submit([encoder.finish()]);
  const failed = device.popErrorScope();
  const target = targetOf(snapshot.target);

  return (async () => {
    const error = await failed;
    if (error) throw new Error(`Texelscope could not read the draw's resources back: ${error.message}`);
    const bindings = await Promise.all(jobs.map((job) => job()));
    const capture: Capture = {
      version: FORMAT_VERSION,
      label: snapshot.label,
      frame: snapshot.frame,
      shader: {
        language: 'wgsl',
        source: { inline: snapshot.code },
        entryPoint: snapshot.entryPoint ?? fragmentEntry(snapshot.code) ?? 'main',
      },
      target,
      inputs,
      bindings: bindings.sort((a, b) => a.group - b.group || a.binding - b.binding),
    };
    if (skipped.length > 0) capture.label = `${capture.label ?? 'capture'} (skipped ${skipped.join('; ')})`;
    return capture;
  })();
}

/** Dynamic offsets apply in binding order to the layout entries that take one. */
function dynamicOffsetsByBinding(entries: GPUBindGroupLayoutEntry[], offsets: number[]): Map<number, number> {
  const dynamic = entries
    .filter((e) => e.buffer?.hasDynamicOffset)
    .map((e) => e.binding)
    .sort((a, b) => a - b);
  return new Map(dynamic.map((binding, i) => [binding, offsets[i] ?? 0]));
}

/** Encodes a copy of a buffer binding and returns how to read it once submitted. */
function readBuffer(
  device: GPUDevice,
  encoder: GPUCommandEncoder,
  binding: GPUBufferBinding,
  dynamicOffset: number,
): () => Promise<Uint8Array> {
  const { buffer } = binding;
  const base = binding.offset ?? 0;
  const offset = base + dynamicOffset;
  const size = binding.size ?? buffer.size - base;
  // Copies move whole words, so the size rounds up when the buffer has room and down when not.
  const copySize = Math.min(align(size, 4), Math.floor((buffer.size - offset) / 4) * 4);
  const staging = device.createBuffer({
    label: 'texelscope buffer readback',
    size: Math.max(copySize, 4),
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  if (copySize > 0) encoder.copyBufferToBuffer(buffer, offset, staging, 0, copySize);
  return () =>
    staging.mapAsync(GPUMapMode.READ).then(() => {
      const bytes = new Uint8Array(staging.getMappedRange(0, copySize).slice(0)).subarray(0, size);
      staging.unmap();
      staging.destroy();
      return bytes;
    });
}

/** Encodes copies of every mip level a view covers and returns how to read them once
 * submitted, or says why it can't. */
function readTexture(
  device: GPUDevice,
  encoder: GPUCommandEncoder,
  view: GPUTextureView,
): (() => Promise<Texture>) | string {
  const known = views.get(view);
  if (!known) return 'its texture was created before Texelscope was installed';
  const { texture, descriptor = {} } = known;
  const format = (descriptor.format ?? texture.format) as TextureFormat;
  const texelBytes = TEXEL_BYTES[format];
  if (!texelBytes) return `${format} textures can't be captured yet`;
  if (texture.sampleCount > 1) return 'multisampled textures can\'t be captured';

  const baseLevel = descriptor.baseMipLevel ?? 0;
  const levels = descriptor.mipLevelCount ?? texture.mipLevelCount - baseLevel;
  const is3d = texture.dimension === '3d';
  const arrayed = descriptor.dimension === '2d-array' || descriptor.dimension === 'cube' || descriptor.dimension === 'cube-array';
  const baseLayer = is3d ? 0 : (descriptor.baseArrayLayer ?? 0);
  const layers = is3d ? texture.depthOrArrayLayers : (descriptor.arrayLayerCount ?? (arrayed ? texture.depthOrArrayLayers - baseLayer : 1));

  const reads: (() => Promise<Uint8Array>)[] = [];
  let size: [number, number, number] = [0, 0, 0];
  for (let level = baseLevel; level < baseLevel + levels; level++) {
    const width = Math.max(1, texture.width >> level);
    const height = texture.dimension === '1d' ? 1 : Math.max(1, texture.height >> level);
    const depth = is3d ? Math.max(1, layers >> level) : layers;
    if (level === baseLevel) size = [width, height, depth];
    const rowBytes = width * texelBytes;
    const bytesPerRow = align(rowBytes, 256);
    const staging = device.createBuffer({
      label: 'texelscope texture readback',
      size: bytesPerRow * height * depth,
      usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
    });
    encoder.copyTextureToBuffer(
      { texture, mipLevel: level, origin: { x: 0, y: 0, z: baseLayer } },
      { buffer: staging, bytesPerRow, rowsPerImage: height },
      { width, height, depthOrArrayLayers: depth },
    );
    reads.push(() =>
      staging.mapAsync(GPUMapMode.READ).then(() => {
        const packed = unpad(new Uint8Array(staging.getMappedRange()), rowBytes, bytesPerRow, height * depth);
        staging.unmap();
        staging.destroy();
        return packed;
      }),
    );
  }
  return () => Promise.all(reads.map((read) => read())).then((parts) => ({
    dimension: texture.dimension === '3d' ? '3d' : texture.dimension === '1d' ? '1d' : '2d',
    format,
    size,
    mipLevelCount: levels,
    data: { base64: toBase64(concat(parts)) },
  }));
}

/** Drops the padding that copies add to make each row a multiple of 256 bytes. */
export function unpad(bytes: Uint8Array, rowBytes: number, bytesPerRow: number, rows: number): Uint8Array {
  if (rowBytes === bytesPerRow) return bytes.slice(0, rowBytes * rows);
  const out = new Uint8Array(rowBytes * rows);
  for (let row = 0; row < rows; row++) {
    out.set(bytes.subarray(row * bytesPerRow, row * bytesPerRow + rowBytes), row * rowBytes);
  }
  return out;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function samplerOf(sampler: GPUSampler): Sampler {
  const d = samplers.get(sampler) ?? {};
  const out: Sampler = {};
  if (d.addressModeU) out.addressModeU = d.addressModeU as Sampler['addressModeU'];
  if (d.addressModeV) out.addressModeV = d.addressModeV as Sampler['addressModeV'];
  if (d.addressModeW) out.addressModeW = d.addressModeW as Sampler['addressModeW'];
  if (d.magFilter) out.magFilter = d.magFilter;
  if (d.minFilter) out.minFilter = d.minFilter;
  if (d.mipmapFilter) out.mipmapFilter = d.mipmapFilter;
  return out;
}

function targetOf(view?: GPUTextureView): Capture['target'] {
  const known = view ? views.get(view) : undefined;
  if (!known) throw new Error('Texelscope could not find the texture the draw renders to');
  const { texture, descriptor = {} } = known;
  const level = descriptor.baseMipLevel ?? 0;
  const format = (descriptor.format ?? texture.format) as TextureFormat;
  return {
    width: Math.max(1, texture.width >> level),
    height: Math.max(1, texture.height >> level),
    format: format in TEXEL_BYTES ? format : 'rgba8unorm',
  };
}
