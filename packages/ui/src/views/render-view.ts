import { LitElement, css, html, nothing, unsafeCSS } from 'lit';
import { css as colorCss, fixed, ramp } from '../format.js';
import type { ExprRef, Rect } from '../types.js';

/** A value the GPU computed at every pixel, shown over the image. */
export interface ProbeView {
  expr: ExprRef;
  name: string;
  components: number;
  /** The channel shown, from 0 to `components - 1`. */
  channel: number;
  /** Four floats per pixel, NaN where a pixel never computed the value. Null while running. */
  data: Float32Array | null;
  min: number[];
  max: number[];
  error: string;
}

const RAMP_CSS = `linear-gradient(90deg, ${[0, 0.25, 0.5, 0.75, 1].map((t) => colorCss([...ramp(t), 1])).join(', ')})`;

/** A layer image drawn over part of the render. */
export interface Overlay {
  rect: Rect;
  image: ImageData;
}

/**
 * The replayed image, zoomed with the wheel and panned by dragging. A click picks a pixel and
 * a shift-drag selects a region.
 */
export class TexelscopeRenderView extends LitElement {
  static override properties = {
    targetWidth: { type: Number },
    targetHeight: { type: Number },
    selected: { attribute: false },
    compared: { attribute: false },
    inputsWindow: { attribute: false },
    region: { attribute: false },
    overlay: { attribute: false },
    probe: { attribute: false },
    message: { type: String },
  };

  static override styles = css`
    :host {
      display: block;
      position: relative;
      overflow: hidden;
      background: repeating-conic-gradient(#e4e6ea 0 25%, #f2f3f5 0 50%) 0 0 / 16px 16px;
      cursor: crosshair;
      user-select: none;
    }
    .stage {
      position: absolute;
      left: 0;
      top: 0;
      transform-origin: 0 0;
    }
    .stage canvas {
      position: absolute;
      left: 0;
      top: 0;
      image-rendering: pixelated;
    }
    .marks {
      position: absolute;
      inset: 0;
      pointer-events: none;
    }
    .message {
      position: absolute;
      inset: 0;
      display: grid;
      place-items: center;
      padding: 24px;
      text-align: center;
      color: #4b5563;
      font-size: 14px;
      line-height: 1.5;
      white-space: pre-line;
    }
    .hud {
      position: absolute;
      right: 8px;
      bottom: 8px;
      padding: 2px 6px;
      border-radius: 4px;
      background: rgba(17, 24, 39, 0.72);
      color: #f9fafb;
      font: 11px ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .legend {
      position: absolute;
      top: 8px;
      left: 8px;
      right: 8px;
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 4px 8px;
      padding: 5px 8px;
      border-radius: 6px;
      background: rgba(255, 255, 255, 0.94);
      box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
      font: 12px system-ui, sans-serif;
      color: #111827;
    }
    .legend code {
      font: 600 12px ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .legend .ramp {
      display: inline-block;
      width: 64px;
      height: 10px;
      border-radius: 2px;
      background: ${unsafeCSS(RAMP_CSS)};
    }
    .legend .dim {
      color: #6b7280;
    }
    .legend .error {
      color: #991b1b;
    }
    .legend button {
      padding: 0 6px;
      border: 1px solid #d1d5db;
      border-radius: 4px;
      background: #fff;
      font: 11px ui-monospace, SFMono-Regular, Menlo, monospace;
      cursor: pointer;
    }
    .legend button.on {
      background: #111827;
      color: #fff;
    }
    .legend .hide {
      margin-left: auto;
      font-family: system-ui, sans-serif;
    }
    .hud.hint {
      left: 8px;
      font: 11.5px system-ui, sans-serif;
      text-align: center;
      pointer-events: none;
    }
  `;

  declare targetWidth: number;
  declare targetHeight: number;
  declare selected: [number, number] | null;
  /** A second pixel the first is compared with. */
  declare compared: [number, number] | null;
  /** Where a WebGL capture recorded its inputs, the only pixels the interpreter can run. */
  declare inputsWindow: Rect | null;
  declare region: Rect | null;
  declare overlay: Overlay | null;
  /** A value shown over the whole image, with its legend. */
  declare probe: ProbeView | null;
  declare message: string;

  #scale = 1;
  #ox = 0;
  #oy = 0;
  #fitted = false;
  #hover: [number, number] | null = null;
  #drag?: { x: number; y: number; ox: number; oy: number; moved: boolean; region: boolean; start: [number, number] };
  #resize = new ResizeObserver(() => this.#draw());

  constructor() {
    super();
    this.targetWidth = 0;
    this.targetHeight = 0;
    this.selected = null;
    this.compared = null;
    this.inputsWindow = null;
    this.region = null;
    this.overlay = null;
    this.probe = null;
    this.message = '';
  }

  /** What the shown value is at a pixel, for the readout. */
  #probeAt([x, y]: [number, number]): string {
    const probe = this.probe;
    if (!probe?.data) return '';
    const at = (y * this.targetWidth + x) * 4;
    const values = Array.from(probe.data.subarray(at, at + probe.components));
    const text = values.some(Number.isNaN) ? 'not computed' : values.map((v) => fixed(v, 4)).join(', ');
    return ` · ${probe.name} = ${text}`;
  }

  #legend() {
    const probe = this.probe;
    if (!probe) return nothing;
    const emit = (name: string, detail: unknown) => this.dispatchEvent(new CustomEvent(name, { detail }));
    const c = probe.channel;
    const lanes = ['x', 'y', 'z', 'w'].slice(0, probe.components);
    return html`<div class="legend" @pointerdown=${(e: Event) => e.stopPropagation()}>
      <span><code>${probe.name}</code> over the whole image</span>
      ${probe.error
        ? html`<span class="error">${probe.error}</span>`
        : !probe.data
          ? html`<span class="dim">computing on the GPU...</span>`
          : html`<span class="dim">${fixed(probe.min[c] ?? 0, 4)}</span><span class="ramp"></span
              ><span class="dim">${fixed(probe.max[c] ?? 0, 4)}</span>
              ${probe.components > 1
                ? lanes.map((lane, k) => html`<button class=${k === c ? 'on' : ''} @click=${() => emit('probe-channel', k)}>${lane}</button>`)
                : nothing}
              <span class="dim">Clear where a pixel never computed it. Hover to read values.</span>`}
      <button class="hide" @click=${() => emit('probe-hide', null)}>Hide</button>
    </div>`;
  }

  /** The canvas the GPU replay presents into. */
  get canvas(): HTMLCanvasElement {
    return this.renderRoot.querySelector('canvas.gpu') as HTMLCanvasElement;
  }

  override connectedCallback() {
    super.connectedCallback();
    this.#resize.observe(this);
  }

  override disconnectedCallback() {
    this.#resize.disconnect();
    super.disconnectedCallback();
  }

  override render() {
    const hud = this.#hover
      ? `${this.#hover[0]}, ${this.#hover[1]}${this.#probeAt(this.#hover)} · ${Math.round(this.#scale * 100)}%`
      : this.targetWidth && !this.message
        ? 'Click a pixel to explain it, then Alt-click another to compare. Shift-drag to inspect a region. Scroll to zoom, drag to pan, double-click to fit.'
        : '';
    return html`
      <div class="stage">
        <canvas class="gpu" width=${this.targetWidth || 1} height=${this.targetHeight || 1}></canvas>
        <canvas class="layer"></canvas>
      </div>
      <canvas class="marks"></canvas>
      ${this.message ? html`<div class="message">${this.message}</div>` : ''}
      ${hud ? html`<div class="hud ${this.#hover ? '' : 'hint'}">${hud}</div>` : ''}
      ${this.#legend()}
    `;
  }

  override firstUpdated() {
    this.addEventListener('wheel', (e) => this.#wheel(e), { passive: false });
    this.addEventListener('pointerdown', (e) => this.#down(e));
    this.addEventListener('pointermove', (e) => this.#move(e));
    this.addEventListener('pointerup', (e) => this.#up(e));
    this.addEventListener('pointerleave', () => {
      this.#hover = null;
      this.requestUpdate();
    });
    this.addEventListener('dblclick', () => this.fit());
  }

  override updated(changed: Map<string, unknown>) {
    if (changed.has('targetWidth') || changed.has('targetHeight')) {
      this.#fitted = false;
      const gpu = this.canvas;
      gpu.style.width = `${this.targetWidth}px`;
      gpu.style.height = `${this.targetHeight}px`;
    }
    if (changed.has('overlay')) this.#paintOverlay();
    this.#draw();
  }

  /** Scales the image to fit the view. */
  fit() {
    const box = this.getBoundingClientRect();
    if (!this.targetWidth || !box.width) return;
    this.#scale = Math.min(box.width / this.targetWidth, box.height / this.targetHeight) * 0.96;
    this.#ox = (box.width - this.targetWidth * this.#scale) / 2;
    this.#oy = (box.height - this.targetHeight * this.#scale) / 2;
    this.#fitted = true;
    this.#draw();
  }

  #toPixel(e: PointerEvent | WheelEvent): [number, number] {
    const box = this.getBoundingClientRect();
    return [Math.floor((e.clientX - box.left - this.#ox) / this.#scale), Math.floor((e.clientY - box.top - this.#oy) / this.#scale)];
  }

  #inside([x, y]: [number, number]) {
    return x >= 0 && y >= 0 && x < this.targetWidth && y < this.targetHeight;
  }

  #wheel(e: WheelEvent) {
    e.preventDefault();
    const box = this.getBoundingClientRect();
    const mx = e.clientX - box.left;
    const my = e.clientY - box.top;
    const next = Math.min(96, Math.max(0.05, this.#scale * Math.exp(-e.deltaY * 0.0015)));
    this.#ox = mx - ((mx - this.#ox) * next) / this.#scale;
    this.#oy = my - ((my - this.#oy) * next) / this.#scale;
    this.#scale = next;
    this.#draw();
  }

  #down(e: PointerEvent) {
    if (e.button !== 0) return;
    this.setPointerCapture(e.pointerId);
    this.#drag = { x: e.clientX, y: e.clientY, ox: this.#ox, oy: this.#oy, moved: false, region: e.shiftKey, start: this.#toPixel(e) };
  }

  #move(e: PointerEvent) {
    const pixel = this.#toPixel(e);
    this.#hover = this.#inside(pixel) ? pixel : null;
    const drag = this.#drag;
    if (drag) {
      const dx = e.clientX - drag.x;
      const dy = e.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      if (drag.moved && drag.region) {
        this.region = this.#rectBetween(drag.start, pixel);
      } else if (drag.moved) {
        this.#ox = drag.ox + dx;
        this.#oy = drag.oy + dy;
      }
    }
    this.#draw();
    this.requestUpdate();
  }

  #up(e: PointerEvent) {
    const drag = this.#drag;
    this.#drag = undefined;
    if (!drag) return;
    const pixel = this.#toPixel(e);
    if (drag.region && drag.moved && this.region) {
      this.dispatchEvent(new CustomEvent('region', { detail: this.region }));
    } else if (!drag.moved && this.#inside(pixel)) {
      // Alt-click picks a second pixel to compare with the first.
      this.dispatchEvent(new CustomEvent(e.altKey && this.selected ? 'compare' : 'pick', { detail: pixel }));
    }
  }

  #rectBetween(a: [number, number], b: [number, number]): Rect {
    const clampX = (v: number) => Math.min(this.targetWidth - 1, Math.max(0, v));
    const clampY = (v: number) => Math.min(this.targetHeight - 1, Math.max(0, v));
    const x0 = clampX(Math.min(a[0], b[0]));
    const y0 = clampY(Math.min(a[1], b[1]));
    const x1 = clampX(Math.max(a[0], b[0]));
    const y1 = clampY(Math.max(a[1], b[1]));
    return { x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 };
  }

  #paintOverlay() {
    const layer = this.renderRoot.querySelector('canvas.layer') as HTMLCanvasElement | null;
    if (!layer) return;
    const overlay = this.overlay;
    if (!overlay) {
      layer.width = 1;
      layer.height = 1;
      layer.style.display = 'none';
      return;
    }
    layer.width = overlay.rect.width;
    layer.height = overlay.rect.height;
    layer.style.display = 'block';
    layer.style.left = `${overlay.rect.x}px`;
    layer.style.top = `${overlay.rect.y}px`;
    layer.style.width = `${overlay.rect.width}px`;
    layer.style.height = `${overlay.rect.height}px`;
    layer.getContext('2d')!.putImageData(overlay.image, 0, 0);
  }

  #draw() {
    if (!this.#fitted && this.targetWidth) this.fit();
    const stage = this.renderRoot.querySelector('.stage') as HTMLElement | null;
    if (stage) stage.style.transform = `translate(${this.#ox}px, ${this.#oy}px) scale(${this.#scale})`;
    const marks = this.renderRoot.querySelector('canvas.marks') as HTMLCanvasElement | null;
    if (!marks) return;
    const box = this.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    marks.width = Math.round(box.width * dpr);
    marks.height = Math.round(box.height * dpr);
    marks.style.width = `${box.width}px`;
    marks.style.height = `${box.height}px`;
    const g = marks.getContext('2d')!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const toScreen = (x: number, y: number): [number, number] => [this.#ox + x * this.#scale, this.#oy + y * this.#scale];
    if (this.inputsWindow) {
      const w = this.inputsWindow;
      const [x, y] = toScreen(w.x, w.y);
      g.strokeStyle = '#d97706';
      g.lineWidth = 1;
      g.setLineDash([2, 3]);
      g.strokeRect(x, y, w.width * this.#scale, w.height * this.#scale);
      g.setLineDash([]);
      g.font = '11px system-ui, sans-serif';
      g.fillStyle = '#b45309';
      g.fillText('inputs recorded here', x + 3, y - 4);
    }
    if (this.region) {
      const [x, y] = toScreen(this.region.x, this.region.y);
      g.strokeStyle = '#2563eb';
      g.lineWidth = 1.5;
      g.setLineDash([5, 4]);
      g.strokeRect(x, y, this.region.width * this.#scale, this.region.height * this.#scale);
      g.setLineDash([]);
    }
    const label = this.compared ? 'A' : '';
    if (this.selected) this.#mark(g, toScreen(this.selected[0], this.selected[1]), '#db2777', label);
    if (this.compared) this.#mark(g, toScreen(this.compared[0], this.compared[1]), '#2563eb', 'B');
  }

  /** A box around one pixel, with a crosshair when zoomed out and an optional letter. */
  #mark(g: CanvasRenderingContext2D, [x, y]: [number, number], color: string, label: string) {
    const size = Math.max(this.#scale, 1);
    const pad = Math.max(3, size * 0.15);
    g.lineWidth = 2;
    g.strokeStyle = '#ffffff';
    g.strokeRect(x - pad, y - pad, size + pad * 2, size + pad * 2);
    g.lineWidth = 1;
    g.strokeStyle = color;
    g.strokeRect(x - pad, y - pad, size + pad * 2, size + pad * 2);
    if (this.#scale < 6) {
      const cx = x + size / 2;
      const cy = y + size / 2;
      g.beginPath();
      g.moveTo(cx - 14, cy);
      g.lineTo(cx - 5, cy);
      g.moveTo(cx + 5, cy);
      g.lineTo(cx + 14, cy);
      g.moveTo(cx, cy - 14);
      g.lineTo(cx, cy - 5);
      g.moveTo(cx, cy + 5);
      g.lineTo(cx, cy + 14);
      g.stroke();
    }
    if (label) {
      g.font = '600 11px system-ui, sans-serif';
      g.lineWidth = 3;
      g.strokeStyle = '#ffffff';
      g.strokeText(label, x + size + pad + 3, y - pad);
      g.fillStyle = color;
      g.fillText(label, x + size + pad + 3, y - pad);
    }
  }
}

customElements.define('texelscope-render-view', TexelscopeRenderView);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-render-view': TexelscopeRenderView;
  }
}
