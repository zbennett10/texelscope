import { LitElement, css, html, nothing } from 'lit';
import { directive, Directive, type ElementPart, type PartInfo, PartType } from 'lit/directive.js';
import { fixed, ramp } from '../format.js';
import type { Layer, RegionData, SourceSpan } from '../types.js';

/** What a layer card shows: one component on a ramp, or the first three as a color. */
export type Channel = number | 'rgb';

/** Paints one layer of a region as an image. NaN, where a pixel never computed it, is clear. */
export function layerImage(region: RegionData, index: number, channel: Channel): ImageData {
  const { width, height } = region.meta.rect;
  const layer = region.meta.layers[index]!;
  const count = width * height;
  const data = region.layers.subarray(index * count * 4, (index + 1) * count * 4);
  const image = new ImageData(width, height);
  const c = channel === 'rgb' ? 0 : channel;
  const lo = layer.min[c] ?? 0;
  const span = (layer.max[c] ?? 1) - lo || 1;
  for (let p = 0; p < count; p++) {
    const v = data[p * 4 + c]!;
    if (Number.isNaN(v)) continue;
    const rgb = channel === 'rgb'
      ? [data[p * 4]!, data[p * 4 + 1]!, data[p * 4 + 2]!]
      : ramp((v - lo) / span);
    image.data.set([rgb[0]! * 255, rgb[1]! * 255, rgb[2]! * 255, 255], p * 4);
  }
  return image;
}

/** Paints one branch: false gray, true green, both orange, never reached clear. */
export function branchImage(region: RegionData, index: number): ImageData {
  const { width, height } = region.meta.rect;
  const count = width * height;
  const data = region.branches.subarray(index * count, (index + 1) * count);
  const image = new ImageData(width, height);
  const colors = [null, [148, 163, 184], [22, 163, 74], [234, 88, 12]];
  for (let p = 0; p < count; p++) {
    const c = colors[data[p]!];
    if (c) image.data.set([c[0]!, c[1]!, c[2]!, 230], p * 4);
  }
  return image;
}

/** Every named value and decision over a region, in the order a pixel computes them. */
export class TexelscopeRegionView extends LitElement {
  static override properties = {
    region: { attribute: false },
    busy: { type: Boolean },
    error: { type: String },
    shown: { type: String },
    source: { type: String },
    channels: { state: true },
  };

  static override styles = css`
    :host {
      display: block;
      overflow: auto;
      background: #fff;
      font: 12px/1.4 system-ui, sans-serif;
      color: #111827;
    }
    header {
      position: sticky;
      top: 0;
      padding: 6px 12px;
      border-bottom: 1px solid #e5e7eb;
      background: #fff;
      color: #6b7280;
    }
    .grid {
      display: flex;
      gap: 10px;
      padding: 10px 12px;
      overflow-x: auto;
    }
    .card {
      flex: 0 0 auto;
      width: 132px;
      padding: 4px;
      border: 1px solid #e5e7eb;
      border-radius: 6px;
      cursor: pointer;
    }
    .card:hover {
      border-color: #9ca3af;
    }
    .card.shown {
      border-color: #db2777;
      box-shadow: 0 0 0 1px #db2777;
    }
    canvas {
      display: block;
      width: 124px;
      height: 90px;
      object-fit: contain;
      image-rendering: pixelated;
      background: repeating-conic-gradient(#eee 0 25%, #fff 0 50%) 0 0 / 10px 10px;
    }
    .name {
      margin-top: 3px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font: 600 11px ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .meta {
      color: #6b7280;
      font: 10.5px ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .chips button {
      padding: 0 4px;
      margin-right: 2px;
      font: 10px ui-monospace, monospace;
      border: 1px solid #d1d5db;
      border-radius: 3px;
      background: #fff;
      cursor: pointer;
    }
    .chips button.on {
      background: #111827;
      color: #fff;
    }
    .empty {
      padding: 12px;
      color: #6b7280;
    }
    .error {
      padding: 12px;
      color: #991b1b;
    }
  `;

  declare region: RegionData | null;
  declare busy: boolean;
  declare error: string;
  /** Which card the render view overlays, as `layer:3` or `branch:1`. */
  declare shown: string;
  /** The source, so cards can name their conditions. */
  declare source: string;
  declare channels: Map<number, Channel>;

  constructor() {
    super();
    this.region = null;
    this.busy = false;
    this.error = '';
    this.shown = '';
    this.source = '';
    this.channels = new Map();
  }

  /** The channel a layer card shows, color when its values look like one. */
  channel(index: number): Channel {
    const chosen = this.channels.get(index);
    if (chosen !== undefined) return chosen;
    const layer = this.region?.meta.layers[index];
    if (!layer) return 0;
    const unit = layer.min.slice(0, 3).every((v) => v >= 0) && layer.max.slice(0, 3).every((v) => v <= 1.001);
    return layer.components >= 3 && unit ? 'rgb' : 0;
  }

  override render() {
    if (this.error) return html`<div class="error">${this.error}</div>`;
    const region = this.region;
    if (!region) {
      return html`<div class="empty">${this.busy ? 'Running every pixel in the region...' : 'Shift-drag over the image to see every named value across a region, step by step from data to color.'}</div>`;
    }
    const { rect, layers, branches } = region.meta;
    return html`
      <header>
        Region ${rect.x}, ${rect.y}, ${rect.width} by ${rect.height}. ${layers.length} values and ${branches.length} decisions, in the order a pixel computes them.
        ${this.busy ? 'Updating...' : ''}
      </header>
      <div class="grid">
        ${layers.map((layer, i) => this.#layerCard(layer, i))}
        ${branches.map((branch, i) => this.#card(`branch:${i}`, branchImage(region, i), branch.span, html`
          <div class="name">${this.#text(branch.span) || 'branch'}</div>
          <div class="meta">true ${branch.taken}, false ${branch.notTaken}</div>
        `))}
      </div>
    `;
  }


  #text(span: SourceSpan | null) {
    return span ? this.source.slice(span.start, span.end).replace(/\s+/g, ' ') : '';
  }

  #layerCard(layer: Layer, i: number) {
    const region = this.region!;
    const channel = this.channel(i);
    const c = channel === 'rgb' ? 0 : channel;
    const range = channel === 'rgb' ? 'as color' : `${fixed(layer.min[c] ?? 0, 3)} to ${fixed(layer.max[c] ?? 0, 3)}`;
    const lanes = ['x', 'y', 'z', 'w'].slice(0, layer.components);
    return this.#card(`layer:${i}`, layerImage(region, i, channel), layer.span, html`
      <div class="name" title=${layer.function}>${layer.name}</div>
      <div class="meta">${range}${layer.evaluations > 1 ? `, last of ${layer.evaluations}` : ''}</div>
      ${layer.components > 1
        ? html`<div class="chips">
            ${lanes.map((lane, k) => html`<button class=${channel === k ? 'on' : ''} @click=${(e: Event) => this.#pick(e, i, k)}>${lane}</button>`)}
            ${layer.components >= 3 ? html`<button class=${channel === 'rgb' ? 'on' : ''} @click=${(e: Event) => this.#pick(e, i, 'rgb')}>rgb</button>` : nothing}
          </div>`
        : nothing}
    `);
  }

  #pick(e: Event, index: number, channel: Channel) {
    e.stopPropagation();
    this.channels = new Map(this.channels).set(index, channel);
    if (this.shown === `layer:${index}`) this.#emit('show', this.shown);
  }

  #card(key: string, image: ImageData, span: SourceSpan | null, body: unknown) {
    return html`<div class="card ${this.shown === key ? 'shown' : ''}"
      @mouseenter=${() => span && this.#emit('highlight', span)}
      @click=${() => this.#emit('show', this.shown === key ? '' : key)}>
      <canvas .width=${image.width} .height=${image.height} ${paint(image)}></canvas>
      ${body}
    </div>`;
  }

  #emit(name: string, detail: unknown) {
    this.dispatchEvent(new CustomEvent(name, { detail, bubbles: true, composed: true }));
  }
}

/** Draws an image into the canvas element it is attached to. */
class PaintDirective extends Directive {
  constructor(part: PartInfo) {
    super(part);
    if (part.type !== PartType.ELEMENT) throw new Error('paint() goes on a canvas element');
  }
  override update(part: ElementPart, [image]: [ImageData]) {
    const canvas = part.element as HTMLCanvasElement;
    canvas.width = image.width;
    canvas.height = image.height;
    canvas.getContext('2d')?.putImageData(image, 0, 0);
    return this.render(image);
  }
  render(_image: ImageData) {
    return nothing;
  }
}
const paint = directive(PaintDirective);

customElements.define('texelscope-region-view', TexelscopeRegionView);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-region-view': TexelscopeRegionView;
  }
}
