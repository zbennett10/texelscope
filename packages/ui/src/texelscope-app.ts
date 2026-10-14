import { LitElement, css, html, nothing } from 'lit';
import type { Capture } from '@texelscope/sdk';
import { Engine } from './engine/client.js';
import { formatValue, lineAt, lineStarts, ramp } from './format.js';
import { Replay } from './gpu/replay.js';
import type {
  Comparison,
  Description,
  Explanation,
  ExprRef,
  Override,
  Rect,
  RegionData,
  SourceSpan,
  Trace,
  Value,
} from './types.js';
import { exampleCapture } from './example.js';
import { dragTo, MIN_WIDTH, paneWidths, type PaneWidths, type Side } from './panes.js';
import { loadPanes, loadTheme, savePanes, saveTheme, type Theme } from './prefs.js';
import { themeTokens } from './theme.js';
import type { InlineValue } from './views/source-view.js';
import { branchImage, layerImage } from './views/region-view.js';
import type { Overlay, ProbeView } from './views/render-view.js';
import './views/inspector.js';
import './views/region-view.js';
import './views/render-view.js';
import './views/source-view.js';

interface Frame {
  capture: Capture;
  json: string;
  received: Date;
}

const other = (side: Side): Side => (side === 'left' ? 'right' : 'left');

/** A ring around one texel. */
const LOGO = html`<svg class="logo" viewBox="0 0 18 18" width="18" height="18" aria-hidden="true">
  <circle cx="9" cy="9" r="7.25" fill="none" stroke="currentColor" stroke-width="1.5" />
  <rect class="texel" x="6.25" y="6.25" width="5.5" height="5.5" rx="1.25" />
</svg>`;

const SUN = html`<svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
  <circle cx="8" cy="8" r="3" />
  <path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M3.4 12.6l1.1-1.1M11.5 4.5l1.1-1.1" />
</svg>`;

const MOON = html`<svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true">
  <path d="M13.4 10.2A5.7 5.7 0 0 1 5.8 2.6a5.7 5.7 0 1 0 7.6 7.6z" />
</svg>`;

/** Texelscope: shader source, the replayed image, and why any pixel or region looks the way it does. */
export class TexelscopeApp extends LitElement {
  static override properties = {
    frames: { state: true },
    current: { state: true },
    status: { state: true },
    description: { state: true },
    source: { state: true },
    sourceError: { state: true },
    pixel: { state: true },
    explanation: { state: true },
    comparePixel: { state: true },
    comparison: { state: true },
    trace: { state: true },
    gpuColor: { state: true },
    inspectError: { state: true },
    step: { state: true },
    highlight: { state: true },
    whatIf: { state: true },
    regionRect: { state: true },
    region: { state: true },
    regionBusy: { state: true },
    regionError: { state: true },
    shown: { state: true },
    probe: { state: true },
    gpuError: { state: true },
    dragging: { state: true },
    theme: { type: String, reflect: true },
  };

  static override styles = [
    themeTokens,
    css`
      :host {
        display: grid;
        /* The side panes' widths come from paneWidths, set on the element as it lays out. */
        grid-template-columns: var(--ts-left, 32%) 0 minmax(0, 1fr) 0 var(--ts-right, 26%);
        grid-template-rows: auto minmax(0, 1fr) 236px;
        height: 100vh;
        font: 13px/1.45 var(--ts-sans);
        color: var(--ts-text);
        background: var(--ts-bg);
        accent-color: var(--ts-accent);
        -webkit-font-smoothing: antialiased;
      }
      :host([resizing]) {
        user-select: none;
      }
      header {
        grid-column: 1 / -1;
        display: flex;
        align-items: center;
        gap: 10px;
        height: 44px;
        box-sizing: border-box;
        padding: 0 10px 0 14px;
        border-bottom: 1px solid var(--ts-border);
        background: var(--ts-header);
      }
      .brand {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-right: 6px;
      }
      .logo {
        flex: none;
        color: var(--ts-text-4);
      }
      .logo .texel {
        fill: var(--ts-accent);
      }
      header h1 {
        margin: 0;
        font-size: 14px;
        font-weight: 600;
        letter-spacing: 0.01em;
      }
      header .dim {
        color: var(--ts-text-4);
        font-size: 12px;
        white-space: nowrap;
      }
      header .status {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--ts-text-3);
        font-size: 12.5px;
      }
      header button,
      header select,
      header label.button {
        height: 28px;
        box-sizing: border-box;
        padding: 0 11px;
        border: 1px solid var(--ts-border-strong);
        border-radius: 7px;
        background: var(--ts-raised);
        color: var(--ts-text);
        font: 500 12px var(--ts-sans);
        cursor: pointer;
        transition: background-color 0.15s, border-color 0.15s, color 0.15s;
      }
      header button,
      header label.button {
        display: inline-flex;
        align-items: center;
        white-space: nowrap;
      }
      header button:hover,
      header select:hover,
      header label.button:hover {
        background: var(--ts-raised-hover);
        border-color: var(--ts-border-hover);
      }
      header button:focus-visible,
      header select:focus-visible,
      header label.button:focus-within {
        outline: 2px solid var(--ts-focus);
        outline-offset: 1px;
      }
      header button.icon {
        justify-content: center;
        width: 28px;
        padding: 0;
        color: var(--ts-text-2);
      }
      header button.icon:hover {
        color: var(--ts-text);
      }
      header input[type='file'] {
        display: none;
      }
      /* Every pane is placed explicitly, since row-locked items would otherwise go first. */
      texelscope-source-view {
        grid-column: 1;
        grid-row: 2 / 4;
        min-width: 0;
        min-height: 0;
      }
      texelscope-render-view {
        grid-column: 3;
        grid-row: 2;
        min-height: 0;
      }
      texelscope-region-view {
        grid-column: 3;
        grid-row: 3;
        border-top: 1px solid var(--ts-border);
        min-height: 0;
      }
      texelscope-inspector {
        grid-column: 5;
        grid-row: 2 / 4;
        min-width: 0;
        min-height: 0;
      }
      /* The edge between two panes. It sits in a column of no width, with its line and grip around it. */
      .splitter {
        position: relative;
        z-index: 3;
        grid-row: 2 / 4;
        outline: none;
        touch-action: none;
      }
      .splitter.left {
        grid-column: 2;
      }
      .splitter.right {
        grid-column: 4;
      }
      .splitter::before {
        content: '';
        position: absolute;
        top: 0;
        bottom: 0;
        left: -1px;
        width: 1px;
        background: var(--ts-border);
        transition: background-color 0.15s, box-shadow 0.15s;
      }
      .splitter::after {
        content: '';
        position: absolute;
        top: 0;
        bottom: 0;
        left: -5px;
        right: -5px;
        cursor: col-resize;
      }
      .splitter:hover::before {
        transition-delay: 0.1s;
      }
      .splitter:hover::before,
      .splitter:focus-visible::before,
      .splitter[data-active]::before {
        background: var(--ts-accent);
        box-shadow: 0 0 0 1px var(--ts-accent);
      }
      /* Covers the page while an edge moves, so the panes under the pointer don't react. */
      .shield {
        display: none;
        position: fixed;
        inset: 0;
        z-index: 10;
        cursor: col-resize;
      }
      :host([resizing]) .shield {
        display: block;
      }
      .drop {
        position: fixed;
        inset: 0;
        z-index: 10;
        display: grid;
        place-items: center;
        background: var(--ts-blue-soft);
        border: 2px dashed var(--ts-blue);
        font-size: 18px;
        color: var(--ts-text);
        pointer-events: none;
      }
    `,
  ];

  declare frames: Frame[];
  declare current: number;
  declare status: string;
  declare description: Description | null;
  declare source: string;
  declare sourceError: string;
  declare pixel: [number, number] | null;
  declare explanation: Explanation | null;
  /** A second pixel to compare the selected one with, picked by Alt-click. */
  declare comparePixel: [number, number] | null;
  declare comparison: Comparison | null;
  declare trace: Trace | null;
  declare gpuColor: number[] | null;
  declare inspectError: string;
  declare step: number | null;
  declare highlight: SourceSpan | null;
  declare whatIf: { override: Override; label: string }[];
  declare regionRect: Rect | null;
  declare region: RegionData | null;
  declare regionBusy: boolean;
  declare regionError: string;
  declare shown: string;
  /** A value the GPU shows over the whole image, while one is shown. */
  declare probe: ProbeView | null;
  #probeToken = 0;
  #probeImage?: { data: Float32Array; channel: number; image: ImageData };
  declare gpuError: string;
  declare dragging: boolean;
  declare theme: Theme;

  /** The side pane widths set by dragging, and the widths in use once fitted to the window. */
  #chosen: Partial<PaneWidths> = loadPanes();
  #widths: PaneWidths = { left: 0, right: 0 };
  #layout = new ResizeObserver(() => this.#fitPanes());

  #engine = new Engine();
  #replay?: Replay;
  #replayReady: Promise<void>;
  /** The source the core last accepted, which the GPU and the interpreter both run. */
  #valid = '';
  #inspectToken = 0;
  #regionToken = 0;

  constructor() {
    super();
    this.frames = [];
    this.current = -1;
    this.status = 'Open a capture, drop one here, or send one from your app with openInTexelscope().';
    this.description = null;
    this.source = '';
    this.sourceError = '';
    this.pixel = null;
    this.explanation = null;
    this.comparePixel = null;
    this.comparison = null;
    this.trace = null;
    this.gpuColor = null;
    this.inspectError = '';
    this.step = null;
    this.highlight = null;
    this.whatIf = [];
    this.regionRect = null;
    this.region = null;
    this.regionBusy = false;
    this.regionError = '';
    this.shown = '';
    this.probe = null;
    this.gpuError = '';
    this.dragging = false;
    this.theme = loadTheme();
    this.#replayReady = Replay.create().then(
      (replay) => {
        this.#replay = replay;
      },
      (error: Error) => {
        this.gpuError = error.message;
      },
    );
  }

  override connectedCallback() {
    super.connectedCallback();
    window.addEventListener('message', this.#onMessage);
    // Tell the page that opened this tab that captures can come now.
    window.opener?.postMessage({ type: 'texelscope:ready' }, '*');
    this.addEventListener('dragover', (e) => {
      e.preventDefault();
      this.dragging = true;
    });
    this.addEventListener('dragleave', () => (this.dragging = false));
    this.addEventListener('drop', (e) => this.#drop(e));
    window.addEventListener('keydown', this.#onKey);
    this.#layout.observe(this);
  }

  override disconnectedCallback() {
    window.removeEventListener('message', this.#onMessage);
    window.removeEventListener('keydown', this.#onKey);
    this.#layout.disconnect();
    super.disconnectedCallback();
  }

  /** Left and right step through frames, so a pixel can be followed over time. */
  #onKey = (e: KeyboardEvent) => {
    const editing = e.composedPath().some((t) => t instanceof HTMLElement && (t.isContentEditable || t.tagName === 'INPUT'));
    if (editing || this.frames.length < 2) return;
    if (e.key === 'ArrowLeft' && this.current > 0) this.#select(this.current - 1);
    if (e.key === 'ArrowRight' && this.current < this.frames.length - 1) this.#select(this.current + 1);
  };

  override render() {
    const frame = this.frames[this.current];
    const target = frame?.capture.target;
    return html`
      <header>
        <div class="brand">${LOGO}<h1>Texelscope</h1></div>
        ${this.frames.length > 1
          ? html`<select @change=${(e: Event) => this.#select(Number((e.target as HTMLSelectElement).value))}>
              ${this.frames.map((f, i) => html`<option value=${i} ?selected=${i === this.current}>
                ${f.capture.frame !== undefined ? `frame ${f.capture.frame}` : `capture ${i + 1}`}, ${f.received.toLocaleTimeString()}
              </option>`)}
            </select>`
          : nothing}
        ${this.frames.length > 1 ? html`<span class="dim">Left and right arrows step frames.</span>` : nothing}
        <span class="status" title=${this.status}>${this.status}</span>
        <label class="button">Open capture<input type="file" accept=".json,application/json" @change=${this.#pickFile} /></label>
        <button @click=${() => this.#receive(exampleCapture(), [100, 128])}>Example</button>
        <button class="icon" title=${this.theme === 'dark' ? 'Light theme' : 'Dark theme'}
          aria-label=${this.theme === 'dark' ? 'Switch to the light theme' : 'Switch to the dark theme'}
          @click=${this.#toggleTheme}>${this.theme === 'dark' ? SUN : MOON}</button>
      </header>
      <texelscope-source-view
        id="source"
        .theme=${this.theme}
        .source=${this.source}
        .highlight=${this.highlight}
        .values=${this.#inlineValues()}
        .notRun=${this.#notRun()}
        .language=${frame?.capture.shader.language ?? 'wgsl'}
        .error=${this.sourceError}
        @edit=${(e: CustomEvent<string>) => this.#edit(e.detail)}
      ></texelscope-source-view>
      ${this.#splitter('left')}
      <texelscope-render-view
        .targetWidth=${target?.width ?? 0}
        .targetHeight=${target?.height ?? 0}
        .selected=${this.pixel}
        .compared=${this.comparePixel}
        .inputsWindow=${frame?.capture.inputs?.kind === 'window' ? frame.capture.inputs.rect : null}
        .region=${this.regionRect}
        .overlay=${this.#overlay()}
        .probe=${this.probe}
        @probe-channel=${(e: CustomEvent<number>) => this.probe && (this.probe = { ...this.probe, channel: e.detail })}
        @probe-hide=${() => this.#hideProbe()}
        .message=${frame ? this.gpuError : 'No capture yet.\nUse Example to try one, or send one from your app.'}
        @pick=${(e: CustomEvent<[number, number]>) => this.#pick(e.detail)}
        @compare=${(e: CustomEvent<[number, number]>) => this.#compare(e.detail)}
        @region=${(e: CustomEvent<Rect>) => this.#inspectRegion(e.detail)}
      ></texelscope-render-view>
      <texelscope-region-view
        .region=${this.region}
        .busy=${this.regionBusy}
        .error=${this.regionError}
        .shown=${this.shown}
        .source=${this.#valid}
        @show=${(e: CustomEvent<string>) => {
          this.shown = e.detail;
          if (e.detail) this.#hideProbe();
        }}
        @probe=${(e: CustomEvent<{ expr: ExprRef; name: string }>) => this.#showProbe(e.detail.expr, e.detail.name)}
        @highlight=${(e: CustomEvent<SourceSpan>) => (this.highlight = e.detail)}
      ></texelscope-region-view>
      ${this.#splitter('right')}
      <texelscope-inspector
        id="inspector"
        .explanation=${this.explanation}
        .comparison=${this.comparison}
        .language=${frame?.capture.shader.language ?? 'wgsl'}
        @stop-compare=${() => this.#compare(null)}
        @probe=${(e: CustomEvent<{ expr: ExprRef; name: string }>) => this.#showProbe(e.detail.expr, e.detail.name)}
        .trace=${this.trace}
        .gpuColor=${this.gpuColor}
        .format=${target?.format ?? 'rgba8unorm'}
        .whatIf=${this.whatIf.map((w) => w.label)}
        .source=${this.#valid}
        .busy=${!!this.pixel && !this.explanation}
        .error=${this.inspectError}
        .step=${this.step}
        @highlight=${(e: CustomEvent<SourceSpan>) => (this.highlight = e.detail)}
        @select-step=${(e: CustomEvent<number>) => this.#selectStep(e.detail)}
        @pin=${(e: CustomEvent<{ expr: ExprRef; value: Value; label: string }>) =>
          this.#addOverride({ kind: 'pin', expr: e.detail.expr, value: e.detail.value }, e.detail.label)}
        @force=${(e: CustomEvent<{ condition: ExprRef; taken: boolean; label: string }>) =>
          this.#addOverride({ kind: 'forceBranch', condition: e.detail.condition, taken: e.detail.taken }, e.detail.label)}
        @remove-override=${(e: CustomEvent<number>) => this.#setOverrides(this.whatIf.filter((_, i) => i !== e.detail))}
        @clear-overrides=${() => this.#setOverrides([])}
      ></texelscope-inspector>
      <div class="shield"></div>
      ${this.dragging ? html`<div class="drop">Drop a capture to open it</div>` : nothing}
    `;
  }

  #toggleTheme = () => {
    this.theme = this.theme === 'dark' ? 'light' : 'dark';
    saveTheme(this.theme);
  };

  /** The edge between a side pane and the image, dragged with the pointer or moved with the arrow keys. */
  #splitter(side: Side) {
    return html`<div class="splitter ${side}" role="separator" tabindex="0" aria-orientation="vertical"
      aria-controls=${side === 'left' ? 'source' : 'inspector'}
      aria-label=${side === 'left' ? 'Source width' : 'Explanation width'}
      aria-valuemin=${MIN_WIDTH[side]}
      @pointerdown=${(e: PointerEvent) => this.#startResize(e, side)}
      @keydown=${(e: KeyboardEvent) => this.#resizeKey(e, side)}
      @dblclick=${() => this.#choosePane(side, undefined)}></div>`;
  }

  /** Lays the side panes out at their chosen widths, fitted to the window. */
  #fitPanes() {
    const width = this.clientWidth;
    if (!width) return;
    this.#widths = paneWidths(this.#chosen, width);
    for (const side of ['left', 'right'] as const) {
      this.style.setProperty(`--ts-${side}`, `${this.#widths[side]}px`);
      const splitter = this.renderRoot.querySelector(`.splitter.${side}`);
      splitter?.setAttribute('aria-valuenow', String(this.#widths[side]));
      splitter?.setAttribute('aria-valuemax', String(Math.max(MIN_WIDTH[side], width - MIN_WIDTH.middle - this.#widths[other(side)])));
    }
  }

  /** Sets one side pane's width, or returns it to its default when given undefined. */
  #choosePane(side: Side, width: number | undefined, save = true) {
    const pick = (s: Side) => (s === side ? width : this.#chosen[s]);
    const left = pick('left');
    const right = pick('right');
    this.#chosen = { ...(left !== undefined && { left }), ...(right !== undefined && { right }) };
    this.#fitPanes();
    if (save) savePanes(this.#chosen);
  }

  #startResize(e: PointerEvent, side: Side) {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    handle.toggleAttribute('data-active', true);
    this.toggleAttribute('resizing', true);
    const box = this.getBoundingClientRect();
    // Where on the edge the pointer took hold, so the edge doesn't jump to the pointer.
    const edge = side === 'left' ? this.#widths.left : box.width - this.#widths.right;
    const grip = e.clientX - box.left - edge;
    const done = new AbortController();
    const signal = done.signal;
    handle.addEventListener(
      'pointermove',
      (move) => this.#choosePane(side, dragTo(side, move.clientX - box.left - grip, box.width, this.#widths[other(side)]), false),
      { signal },
    );
    const end = () => {
      done.abort();
      handle.removeAttribute('data-active');
      // Focus left on the edge would take the arrow keys, which step through frames.
      handle.blur();
      this.removeAttribute('resizing');
      savePanes(this.#chosen);
    };
    handle.addEventListener('pointerup', end, { signal });
    handle.addEventListener('lostpointercapture', end, { signal });
  }

  #resizeKey(e: KeyboardEvent, side: Side) {
    const step = e.shiftKey ? 64 : 16;
    const move = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
    if (!move) return;
    // The arrow keys also step through frames, which they shouldn't while an edge has focus.
    e.preventDefault();
    e.stopPropagation();
    const width = this.clientWidth;
    const edge = side === 'left' ? this.#widths.left : width - this.#widths.right;
    this.#choosePane(side, dragTo(side, edge + move, width, this.#widths[other(side)]));
  }

  #onMessage = (event: MessageEvent) => {
    const data = event.data as { type?: string; capture?: Capture; pixel?: [number, number] } | undefined;
    // An app that reloaded finds this tab again and asks whether it is listening.
    if (data?.type === 'texelscope:hello') {
      (event.source as Window | null)?.postMessage({ type: 'texelscope:ready' }, event.origin);
    }
    if (data?.type === 'texelscope:capture' && data.capture) this.#receive(data.capture, data.pixel);
  };

  async #pickFile(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (file) this.#receive(JSON.parse(await file.text()) as Capture);
  }

  async #drop(e: DragEvent) {
    e.preventDefault();
    this.dragging = false;
    const file = e.dataTransfer?.files[0];
    if (!file) return;
    try {
      this.#receive(JSON.parse(await file.text()) as Capture);
    } catch (error) {
      this.status = `${file.name} is not capture JSON: ${(error as Error).message}`;
    }
  }

  /** Adds a capture to the frame list and opens it. */
  #receive(capture: Capture, pixel?: [number, number]) {
    const frame: Frame = { capture, json: JSON.stringify(capture), received: new Date() };
    this.frames = [...this.frames, frame].slice(-24);
    this.#select(this.frames.length - 1, pixel);
  }

  async #select(index: number, pixel?: [number, number]) {
    const frame = this.frames[index];
    if (!frame) return;
    this.current = index;
    const shader = frame.capture.shader.source;
    if (!('inline' in shader)) {
      this.status = 'This capture names its shader by path. The browser needs the source inline, as the SDK writes it.';
      return;
    }
    this.status = 'Loading capture...';
    let description: Description;
    try {
      description = await this.#engine.load(frame.json);
    } catch (error) {
      this.status = `Could not load the capture: ${(error as Error).message}`;
      return;
    }
    // A new shader invalidates overrides, which point at its expressions.
    if (shader.inline !== this.#valid) this.whatIf = [];
    this.description = description;
    this.source = shader.inline;
    this.#valid = shader.inline;
    this.sourceError = '';
    const t = frame.capture.target;
    const bindings = frame.capture.bindings?.length ?? 0;
    this.status = `${frame.capture.label ?? 'capture'}: ${frame.capture.shader.entryPoint}, ${t.width} by ${t.height} ${t.format}, ${bindings} bindings`;
    await this.#replayReady;
    if (this.#replay) {
      await this.#replay.load(frame.capture, this.#engine);
      await this.updateComplete;
      this.#replay.attach(this.renderRoot.querySelector('texelscope-render-view')!.canvas);
    }
    await this.#refresh(pixel ?? this.pixel ?? undefined);
  }

  /** Renders on the GPU and re-inspects the pixel and region with the current overrides. */
  async #refresh(pixel?: [number, number]) {
    if (this.probe) void this.#showProbe(this.probe.expr, this.probe.name);
    await this.#renderGpu();
    if (pixel) await this.#pick(pixel);
    if (this.regionRect) await this.#inspectRegion(this.regionRect);
  }

  async #renderGpu() {
    if (!this.#replay || !this.description) return;
    const overrides = this.whatIf.map((w) => w.override);
    // A WebGL capture shows the frame the app drew, which what-ifs can't change.
    if (this.frames[this.current]?.capture.shader.language === 'glsl') {
      await this.#replay.render(this.#valid, this.description);
      this.gpuError = overrides.length > 0
        ? "This is your app's frame. What-ifs show in the explanation, since WebGL captures can't be replayed on the GPU yet."
        : '';
      return;
    }
    let source = this.#valid;
    if (overrides.length > 0) {
      try {
        source = await this.#engine.patched(overrides);
      } catch (error) {
        this.gpuError = `The GPU can't show these overrides: ${(error as Error).message}`;
      }
    }
    const error = await this.#replay.render(source, this.description);
    this.gpuError = error ? `The GPU replay failed:\n${error}` : '';
  }

  async #pick(pixel: [number, number]) {
    const token = ++this.#inspectToken;
    this.pixel = pixel;
    // The last explanation stays up until the new one arrives, so the inspector keeps its scroll.
    this.inspectError = '';
    const overrides = this.whatIf.map((w) => w.override);
    try {
      const [explanation, trace, gpu] = await Promise.all([
        this.#engine.explain(pixel[0], pixel[1], overrides),
        this.#engine.trace(pixel[0], pixel[1], overrides),
        this.#replay?.readPixel(pixel[0], pixel[1]) ?? Promise.resolve(null),
      ]);
      if (token !== this.#inspectToken) return;
      this.explanation = explanation;
      this.trace = trace;
      this.gpuColor = gpu && gpu.length ? gpu : null;
      this.step = trace.result;
    } catch (error) {
      if (token === this.#inspectToken) this.inspectError = (error as Error).message;
    }
    if (this.comparePixel) await this.#compare(this.comparePixel);
  }

  /** Shows one value over the whole image, computed on the GPU by a rewritten shader. */
  async #showProbe(expr: ExprRef, name: string) {
    this.shown = '';
    const token = ++this.#probeToken;
    this.probe = { expr, name, components: 1, channel: 0, data: null, min: [], max: [], error: '' };
    const overrides = this.whatIf.map((w) => w.override);
    try {
      const { wgsl, components } = await this.#engine.probe(expr, overrides);
      const data = this.#replay && this.description
        ? await this.#replay.probe(wgsl, this.description)
        : 'WebGPU is not available, so values can only be shown for a region.';
      if (token !== this.#probeToken || !this.probe) return;
      if (typeof data === 'string') {
        this.probe = { ...this.probe, error: data };
        return;
      }
      const { min, max } = ranges(data, components);
      this.probe = { ...this.probe, components, data, min, max };
    } catch (error) {
      if (token === this.#probeToken && this.probe) this.probe = { ...this.probe, error: (error as Error).message };
    }
  }

  #hideProbe() {
    ++this.#probeToken;
    this.probe = null;
  }

  /** The probe as an image, kept until the data or channel changes. */
  #probeOverlay(probe: ProbeView, width: number, height: number): ImageData {
    const cached = this.#probeImage;
    if (cached && cached.data === probe.data && cached.channel === probe.channel) return cached.image;
    const data = probe.data!;
    const c = probe.channel;
    const lo = probe.min[c] ?? 0;
    const span = (probe.max[c] ?? 1) - lo || 1;
    const image = new ImageData(width, height);
    for (let p = 0; p < width * height; p++) {
      const v = data[p * 4 + c]!;
      if (Number.isNaN(v)) continue;
      const [r, g, b] = ramp((v - lo) / span);
      image.data.set([r * 255, g * 255, b * 255, 255], p * 4);
    }
    this.#probeImage = { data, channel: c, image };
    return image;
  }

  /** Compares the selected pixel with another, or stops comparing when given null. */
  async #compare(other: [number, number] | null) {
    this.comparePixel = other;
    const pixel = this.pixel;
    if (!other || !pixel) {
      this.comparison = null;
      return;
    }
    try {
      this.comparison = await this.#engine.compare(pixel, other, this.whatIf.map((w) => w.override));
    } catch (error) {
      this.comparison = null;
      this.inspectError = (error as Error).message;
    }
  }

  async #inspectRegion(rect: Rect) {
    const token = ++this.#regionToken;
    this.regionRect = rect;
    this.regionBusy = true;
    this.regionError = '';
    try {
      const region = await this.#engine.region(rect, this.whatIf.map((w) => w.override), null);
      if (token !== this.#regionToken) return;
      this.region = region;
      if (this.shown && !this.#overlay()) this.shown = '';
    } catch (error) {
      if (token === this.#regionToken) this.regionError = (error as Error).message;
    } finally {
      if (token === this.#regionToken) this.regionBusy = false;
    }
  }

  async #edit(text: string) {
    this.source = text;
    try {
      this.description = await this.#engine.setSource(text);
    } catch (error) {
      this.sourceError = (error as Error).message;
      return;
    }
    this.sourceError = '';
    this.#valid = text;
    // Edits renumber expressions, so overrides that point at them no longer hold.
    this.whatIf = [];
    if (this.#replay) await this.#replay.uploadBuffers(this.#engine);
    await this.#refresh(this.pixel ?? undefined);
  }

  #addOverride(override: Override, label: string) {
    const key = (o: Override) => JSON.stringify(o.kind === 'forceBranch' ? o.condition : o.expr);
    const rest = this.whatIf.filter((w) => key(w.override) !== key(override));
    this.#setOverrides([...rest, { override, label }]);
  }

  async #setOverrides(next: { override: Override; label: string }[]) {
    this.whatIf = next;
    await this.#refresh(this.pixel ?? undefined);
  }

  #selectStep(index: number) {
    this.step = index;
    const step = this.trace?.steps[index];
    if (step?.span) this.highlight = step.span;
  }

  /** The last value of each named value on its line, from the current trace. */
  /** Lines of code the selected pixel never ran, so the source view can dim them. */
  #notRun(): number[] {
    const trace = this.trace;
    const code = this.description?.codeLines;
    if (!trace || !code) return [];
    const starts = lineStarts(this.#valid);
    const run = new Set<number>();
    const mark = (span: SourceSpan | null | undefined) => {
      if (!span) return;
      const last = lineAt(starts, Math.max(span.start, span.end - 1));
      for (let line = lineAt(starts, span.start); line <= last; line++) run.add(line);
    };
    for (const step of trace.steps) mark(step.span);
    for (const store of trace.stores) mark(store.span);
    for (const branch of trace.branches) mark(branch.span);
    return code.filter((line) => !run.has(line));
  }

  #inlineValues(): InlineValue[] {
    const trace = this.trace;
    if (!trace) return [];
    const starts = lineStarts(this.#valid);
    const lineOf = (offset: number) => lineAt(starts, offset);
    const byLine = new Map<number, Map<string, { text: string; count: number }>>();
    const put = (span: SourceSpan | null, name: string, text: string) => {
      if (!span) return;
      const line = lineOf(span.start);
      const names = byLine.get(line) ?? new Map();
      const prior = names.get(name);
      names.set(name, { text, count: (prior?.count ?? 0) + 1 });
      byLine.set(line, names);
    };
    for (const step of trace.steps) if (step.name) put(step.span, step.name, formatValue(step.value));
    for (const store of trace.stores) put(store.span, store.target, formatValue(store.value));
    return [...byLine].map(([line, names]) => ({
      line,
      text: [...names]
        .map(([name, { text, count }]) => `${name} = ${text}${count > 1 ? ` (last of ${count})` : ''}`)
        .join('   ')
        .slice(0, 160),
    }));
  }

  #overlay(): Overlay | null {
    const probe = this.probe;
    const target = this.frames[this.current]?.capture.target;
    if (probe?.data && target) {
      return { rect: { x: 0, y: 0, width: target.width, height: target.height }, image: this.#probeOverlay(probe, target.width, target.height) };
    }
    const region = this.region;
    if (!region || !this.shown) return null;
    const [kind, at] = this.shown.split(':');
    const index = Number(at);
    const rect = region.meta.rect;
    if (kind === 'layer' && region.meta.layers[index]) {
      const view = this.renderRoot.querySelector('texelscope-region-view');
      return { rect, image: layerImage(region, index, view?.channel(index) ?? 0) };
    }
    if (kind === 'branch' && region.meta.branches[index]) return { rect, image: branchImage(region, index) };
    return null;
  }
}

customElements.define('texelscope-app', TexelscopeApp);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-app': TexelscopeApp;
  }
}

/** The smallest and largest finite value of each channel. */
function ranges(data: Float32Array, components: number): { min: number[]; max: number[] } {
  const min = Array(components).fill(Infinity);
  const max = Array(components).fill(-Infinity);
  for (let p = 0; p < data.length; p += 4) {
    for (let c = 0; c < components; c++) {
      const v = data[p + c]!;
      if (!Number.isFinite(v)) continue;
      if (v < min[c]) min[c] = v;
      if (v > max[c]) max[c] = v;
    }
  }
  return { min: min.map((v) => (Number.isFinite(v) ? v : 0)), max: max.map((v) => (Number.isFinite(v) ? v : 0)) };
}
