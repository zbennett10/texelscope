import { LitElement, css, html } from 'lit';
import type { Rect } from '../types.js';

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
    region: { attribute: false },
    overlay: { attribute: false },
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
  `;

  declare targetWidth: number;
  declare targetHeight: number;
  declare selected: [number, number] | null;
  declare region: Rect | null;
  declare overlay: Overlay | null;
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
    this.region = null;
    this.overlay = null;
    this.message = '';
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
    const pixel = this.#hover ? `${this.#hover[0]}, ${this.#hover[1]}` : '';
    return html`
      <div class="stage">
        <canvas class="gpu" width=${this.targetWidth || 1} height=${this.targetHeight || 1}></canvas>
        <canvas class="layer"></canvas>
      </div>
      <canvas class="marks"></canvas>
      ${this.message ? html`<div class="message">${this.message}</div>` : ''}
      ${pixel ? html`<div class="hud">${pixel} · ${Math.round(this.#scale * 100)}%</div>` : ''}
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
      this.dispatchEvent(new CustomEvent('pick', { detail: pixel }));
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
    if (this.region) {
      const [x, y] = toScreen(this.region.x, this.region.y);
      g.strokeStyle = '#2563eb';
      g.lineWidth = 1.5;
      g.setLineDash([5, 4]);
      g.strokeRect(x, y, this.region.width * this.#scale, this.region.height * this.#scale);
      g.setLineDash([]);
    }
    if (this.selected) {
      const [x, y] = toScreen(this.selected[0], this.selected[1]);
      const size = Math.max(this.#scale, 1);
      const pad = Math.max(3, size * 0.15);
      g.lineWidth = 2;
      g.strokeStyle = '#ffffff';
      g.strokeRect(x - pad, y - pad, size + pad * 2, size + pad * 2);
      g.lineWidth = 1;
      g.strokeStyle = '#db2777';
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
    }
  }
}

customElements.define('texelscope-render-view', TexelscopeRenderView);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-render-view': TexelscopeRenderView;
  }
}
