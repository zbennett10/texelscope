import { LitElement, css, html, nothing, unsafeCSS } from 'lit';
import { directive, Directive, type ElementPart, type PartInfo, PartType } from 'lit/directive.js';
import { css as colorCss, fixed, ramp } from '../format.js';
import type { BranchLayer, Layer, RegionData, SourceSpan } from '../types.js';
import { arrange, type Card } from './region-cards.js';
import { findEdge, type Edge } from './region-edge.js';

/** What a layer card shows: one component on a ramp, or the first three as a color. */
export type Channel = number | 'rgb';

/** Decision colors by the per-pixel code: never reached, false, true, both. */
const DECISION_RGB = [null, [148, 163, 184], [22, 163, 74], [234, 88, 12]] as const;

/** The value ramp as a CSS gradient, for the key. */
const RAMP_CSS = `linear-gradient(90deg, ${[0, 0.25, 0.5, 0.75, 1].map((t) => colorCss([...ramp(t), 1])).join(', ')})`;

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
  for (let p = 0; p < count; p++) {
    const c = DECISION_RGB[data[p]!];
    if (c) image.data.set([c[0], c[1], c[2], 230], p * 4);
  }
  return image;
}

/** A count with its noun, as in "1 decision" or "3 values". */
function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}

/** Whether a layer's values look like a color, with three or four components in 0 to 1. */
function colorLike(layer: Layer): boolean {
  const unit = layer.min.slice(0, 3).every((v) => v >= 0) && layer.max.slice(0, 3).every((v) => v <= 1.001);
  return layer.components >= 3 && unit;
}

/**
 * Every named value and decision over a region. What differs between pixels shows as images
 * in compute order, and what every pixel shares is listed beside them.
 */
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
      display: grid;
      grid-template-rows: auto minmax(0, 1fr);
      overflow: hidden;
      background: var(--ts-surface);
      font: 12px/1.45 var(--ts-sans);
      color: var(--ts-text);
    }
    header {
      padding: 7px 14px;
      border-bottom: 1px solid var(--ts-border);
      color: var(--ts-text-2);
    }
    .key {
      display: flex;
      flex-wrap: wrap;
      gap: 2px 12px;
      margin-top: 3px;
      color: var(--ts-text-3);
      font-size: 11px;
    }
    .ramp {
      display: inline-block;
      width: 44px;
      height: 9px;
      margin-right: 4px;
      border-radius: 2px;
      vertical-align: -1px;
      background: ${unsafeCSS(RAMP_CSS)};
    }
    .dot {
      display: inline-block;
      width: 9px;
      height: 9px;
      margin-right: 3px;
      border-radius: 2px;
      vertical-align: -1px;
    }
    .dot.true {
      background: rgb(22, 163, 74);
    }
    .dot.false {
      background: rgb(148, 163, 184);
    }
    .dot.both {
      background: rgb(234, 88, 12);
    }
    .dot.none {
      box-sizing: border-box;
      border: 1px solid var(--ts-border-strong);
      background: repeating-conic-gradient(var(--ts-swatch-a) 0 25%, var(--ts-swatch-b) 0 50%) 0 0 / 4px 4px;
    }
    .body {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(210px, 32%);
      min-height: 0;
    }
    .varying {
      min-width: 0;
      overflow: hidden;
    }
    .same {
      min-height: 0;
      overflow: auto;
      padding-bottom: 8px;
      border-left: 1px solid var(--ts-border);
      background: var(--ts-surface-2);
    }
    h3 {
      margin: 8px 14px 0;
      font-size: 10.5px;
      font-weight: 600;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--ts-text-3);
    }
    .grid {
      display: flex;
      gap: 8px;
      padding: 6px 14px 10px;
      overflow-x: auto;
    }
    .card {
      flex: 0 0 auto;
      width: 120px;
      padding: 4px;
      border: 1px solid var(--ts-border);
      border-radius: 8px;
      background: var(--ts-surface-2);
      cursor: pointer;
      transition: border-color 0.15s, box-shadow 0.15s;
    }
    .card:hover {
      border-color: var(--ts-border-hover);
    }
    .card.shown {
      border-color: var(--ts-accent);
      box-shadow: 0 0 0 1px var(--ts-accent);
    }
    .card.edge {
      border-color: var(--ts-amber);
    }
    .starts {
      margin: -1px 0 3px;
      color: var(--ts-amber-text);
      font: 600 10px var(--ts-sans);
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }
    .edge-note {
      margin-top: 3px;
      color: var(--ts-amber-text);
      font-size: 11.5px;
    }
    .edge-note .swatch {
      margin: 0 2px;
    }
    button.link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--ts-amber-text);
      font: 600 11.5px var(--ts-mono);
      text-decoration: underline;
      cursor: pointer;
    }
    canvas {
      display: block;
      width: 112px;
      height: 76px;
      object-fit: contain;
      border-radius: 4px;
      image-rendering: pixelated;
      background: repeating-conic-gradient(var(--ts-swatch-a) 0 25%, var(--ts-swatch-b) 0 50%) 0 0 / 10px 10px;
    }
    .name {
      margin-top: 4px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font: 600 11px var(--ts-mono);
    }
    .meta {
      color: var(--ts-text-3);
      font: 10.5px var(--ts-mono);
    }
    .meta .dot {
      width: 8px;
      height: 8px;
      margin-left: 4px;
    }
    .meta .dot:first-child {
      margin-left: 0;
    }
    .chips button {
      padding: 0 5px;
      margin-right: 2px;
      font: 10px var(--ts-mono);
      border: 1px solid var(--ts-border-strong);
      border-radius: 4px;
      background: var(--ts-raised);
      color: var(--ts-text-2);
      cursor: pointer;
    }
    .chips button:hover {
      border-color: var(--ts-border-hover);
      color: var(--ts-text);
    }
    .chips button.on {
      border-color: var(--ts-text);
      background: var(--ts-text);
      color: var(--ts-surface);
    }
    .chips button.map {
      border-color: var(--ts-accent);
      color: var(--ts-accent-text);
    }
    .item {
      display: flex;
      gap: 6px;
      align-items: baseline;
      margin: 0 6px;
      padding: 1px 8px;
      border-radius: 5px;
      font: 11px/1.6 var(--ts-mono);
    }
    .item:hover {
      background: var(--ts-accent-soft);
    }
    .item .label {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .item .value {
      white-space: nowrap;
    }
    .item.setting {
      color: var(--ts-text-4);
    }
    .swatch {
      display: inline-block;
      width: 10px;
      height: 10px;
      margin-right: 4px;
      border: 1px solid var(--ts-swatch-border);
      border-radius: 2px;
      vertical-align: -1px;
    }
    .sub {
      margin: 8px 14px 2px;
      color: var(--ts-text-4);
      font-size: 11px;
    }
    .note,
    .empty {
      padding: 6px 14px;
      color: var(--ts-text-3);
    }
    .empty {
      padding: 14px;
      line-height: 1.65;
    }
    .error {
      padding: 14px;
      color: var(--ts-red-text);
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
  /** The edge found in the last region, kept so redraws don't search again. */
  #edge?: { region: RegionData; edge: Edge | null };

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
    return layer && colorLike(layer) ? 'rgb' : 0;
  }

  override render() {
    if (this.error) return html`<div class="error">${this.error}</div>`;
    const region = this.region;
    if (!region) {
      return html`<div class="empty">
        ${this.busy
          ? 'Running every pixel in the region...'
          : 'Shift-drag over the image to pick a region. Every value the shader computes there shows up here as a small image, in the order a pixel computes them, so you can see the picture build up from data to color.'}
      </div>`;
    }
    const { rect, layers, branches } = region.meta;
    const { varying, same, settings } = arrange(region.meta);
    if (this.#edge?.region !== region) this.#edge = { region, edge: findEdge(region, varying) };
    const edge = this.#edge.edge;
    return html`
      <header>
        <div>
          Region at ${rect.x}, ${rect.y}, ${rect.width} by ${rect.height} pixels: ${count(layers.length, 'value')} and
          ${count(branches.length, 'decision')}. Click a card to lay it over the image. ${this.busy ? 'Updating...' : ''}
        </div>
        <div class="key">
          <span><span class="ramp"></span>value, lowest to highest in this region</span>
          <span><span class="dot true"></span>true</span>
          <span><span class="dot false"></span>false</span>
          <span><span class="dot both"></span>both, in a loop</span>
          <span><span class="dot none"></span>not computed</span>
        </div>
        ${edge
          ? html`<div class="edge-note">
              The edge here, between ${this.#swatch(edge.colors[0])} and ${this.#swatch(edge.colors[1])},
              ${edge.start === edge.full
                ? html`first shows in ${this.#cardLink(edge.start)}.`
                : html`starts to show in ${this.#cardLink(edge.start)} and is complete by ${this.#cardLink(edge.full)}.`}
            </div>`
          : nothing}
      </header>
      <div class="body">
        <section class="varying">
          <h3>Changes across the region (${varying.length})</h3>
          ${varying.length > 0
            ? html`<div class="grid">${varying.map((card) => this.#card(card, edge))}</div>`
            : html`<div class="note">Nothing changes from pixel to pixel here. Every value is listed on the right.</div>`}
        </section>
        <section class="same">
          <h3>Same at every pixel (${same.length + settings.length})</h3>
          ${same.map((card) => this.#item(card, false))}
          ${settings.length > 0
            ? html`<div class="sub">From settings, the same everywhere</div>${settings.map((card) => this.#item(card, true))}`
            : nothing}
        </section>
      </div>
    `;
  }

  #text(span: SourceSpan | null) {
    return span ? this.source.slice(span.start, span.end).replace(/\s+/g, ' ') : '';
  }

  #swatch(rgb: number[]) {
    const color = rgb.some((c) => c < 0) ? 'transparent' : colorCss([...rgb, 1]);
    return html`<span class="swatch" style="background: ${color}"></span>`;
  }

  #cardName(card: Card): string {
    const meta = this.region!.meta;
    if (card.kind === 'layer') return meta.layers[card.index]!.name;
    return this.#text(meta.branches[card.index]!.span) || 'a decision';
  }

  #cardLink(card: Card) {
    return html`<button class="link" title="Show this card" @click=${() => this.#goTo(card)}>${this.#cardName(card)}</button>`;
  }

  /** Scrolls to a card and lays it over the image. */
  #goTo(card: Card) {
    const key = `${card.kind}:${card.index}`;
    this.renderRoot.querySelector(`[data-key="${key}"]`)?.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' });
    this.#emit('show', key);
  }

  #card(card: Card, edge: Edge | null) {
    const region = this.region!;
    const is = (other?: Card) => other?.kind === card.kind && other.index === card.index;
    const starts = is(edge?.start) ? 'edge starts here' : is(edge?.full) ? 'edge complete' : '';
    if (card.kind === 'branch') {
      const branch = region.meta.branches[card.index]!;
      return this.#frame(`branch:${card.index}`, branchImage(region, card.index), branch.span, starts, html`
        <div class="name" title=${this.#text(branch.span)}>${this.#text(branch.span) || 'decision'}</div>
        <div class="meta">${this.#counts(branch)}</div>
      `);
    }
    const i = card.index;
    const layer = region.meta.layers[i]!;
    const channel = this.channel(i);
    const c = channel === 'rgb' ? 0 : channel;
    const pixels = region.meta.rect.width * region.meta.rect.height;
    const range = channel === 'rgb' ? 'shown as color' : `${fixed(layer.min[c] ?? NaN, 3)} to ${fixed(layer.max[c] ?? NaN, 3)}`;
    const notes = [
      layer.evaluations > 1 ? `last of ${layer.evaluations} passes` : '',
      layer.reached < pixels ? `${layer.reached} of ${pixels} px` : '',
    ].filter(Boolean);
    const lanes = ['x', 'y', 'z', 'w'].slice(0, layer.components);
    return this.#frame(`layer:${i}`, layerImage(region, i, channel), layer.span, starts, html`
      <div class="name" title=${layer.function ? `${layer.name} in ${layer.function}` : layer.name}>${layer.name}</div>
      <div class="meta" title=${range}>${range}</div>
      ${notes.length > 0 ? html`<div class="meta">${notes.join(', ')}</div>` : nothing}
      <div class="chips">
        ${layer.components > 1
          ? html`${lanes.map((lane, k) => html`<button class=${channel === k ? 'on' : ''} @click=${(e: Event) => this.#pick(e, i, k)}>${lane}</button>`)}
            ${layer.components >= 3 ? html`<button class=${channel === 'rgb' ? 'on' : ''} @click=${(e: Event) => this.#pick(e, i, 'rgb')}>rgb</button>` : nothing}`
          : nothing}
        ${layer.expr
          ? html`<button class="map" title="Show this value at every pixel of the image, computed on the GPU"
              @click=${(e: Event) => {
                e.stopPropagation();
                this.#emit('probe', { expr: layer.expr, name: layer.name });
              }}>map</button>`
          : nothing}
      </div>
    `);
  }

  /** How many pixels went each way, with the key's colors. */
  #counts(branch: BranchLayer) {
    const pixels = this.region!.meta.rect.width * this.region!.meta.rect.height;
    const missed = pixels - branch.taken - branch.notTaken - branch.mixed;
    return html`<span title="pixels that went each way"
      ><span class="dot true"></span>${branch.taken}<span class="dot false"></span>${branch.notTaken}${branch.mixed > 0
        ? html`<span class="dot both"></span>${branch.mixed}`
        : nothing}${missed > 0 ? html`<span class="dot none"></span>${missed}` : nothing}</span
    >`;
  }

  /** One line for a value or decision that every pixel shares. */
  #item(card: Card, setting: boolean) {
    const meta = this.region!.meta;
    if (card.kind === 'branch') {
      const branch = meta.branches[card.index]!;
      const text = this.#text(branch.span) || 'decision';
      const taken = branch.taken > 0;
      return html`<div class="item ${setting ? 'setting' : ''}" title=${text} @mouseenter=${() => this.#highlight(branch.span)}>
        <span class="label">${text}</span>
        <span class="value"><span class="dot ${taken ? 'true' : 'false'}"></span>${taken ? 'true' : 'false'}</span>
      </div>`;
    }
    const layer = meta.layers[card.index]!;
    const parts = layer.min.slice(0, layer.components).map((v) => fixed(v ?? NaN, 4));
    const value = parts.length === 1 ? parts[0] : `(${parts.join(', ')})`;
    const swatch = colorLike(layer)
      ? html`<span class="swatch" style="background: ${colorCss([...layer.min.slice(0, 3), 1])}"></span>`
      : nothing;
    return html`<div class="item ${setting ? 'setting' : ''}" title=${layer.function ? `${layer.name} in ${layer.function}` : layer.name}
      @mouseenter=${() => this.#highlight(layer.span)}>
      <span class="label">${layer.name}</span>
      <span class="value">${swatch}${value}</span>
    </div>`;
  }

  #pick(e: Event, index: number, channel: Channel) {
    e.stopPropagation();
    this.channels = new Map(this.channels).set(index, channel);
    if (this.shown === `layer:${index}`) this.#emit('show', this.shown);
  }

  #frame(key: string, image: ImageData, span: SourceSpan | null, edge: string, body: unknown) {
    return html`<div class="card ${this.shown === key ? 'shown' : ''} ${edge ? 'edge' : ''}" data-key=${key}
      @mouseenter=${() => this.#highlight(span)}
      @click=${() => this.#emit('show', this.shown === key ? '' : key)}>
      ${edge ? html`<div class="starts">${edge}</div>` : nothing}
      <canvas .width=${image.width} .height=${image.height} ${paint(image)}></canvas>
      ${body}
    </div>`;
  }

  #highlight(span: SourceSpan | null) {
    if (span) this.#emit('highlight', span);
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
