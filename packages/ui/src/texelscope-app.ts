import { LitElement, css, html, nothing } from 'lit';
import type { Capture } from '@texelscope/sdk';
import { Engine } from './engine/client.js';
import { formatValue } from './format.js';
import { Replay } from './gpu/replay.js';
import type { Description, Explanation, ExprRef, Override, Rect, RegionData, SourceSpan, Trace, Value } from './types.js';
import { exampleCapture } from './example.js';
import type { InlineValue } from './views/source-view.js';
import { branchImage, layerImage } from './views/region-view.js';
import type { Overlay } from './views/render-view.js';
import './views/inspector.js';
import './views/region-view.js';
import './views/render-view.js';
import './views/source-view.js';

interface Frame {
  capture: Capture;
  json: string;
  received: Date;
}

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
    gpuError: { state: true },
    dragging: { state: true },
  };

  static override styles = css`
    :host {
      display: grid;
      grid-template-columns: minmax(280px, 1fr) minmax(320px, 1.35fr) minmax(320px, 400px);
      grid-template-rows: auto minmax(0, 1fr) 210px;
      height: 100vh;
      font: 13px/1.4 system-ui, sans-serif;
      color: #111827;
      background: #f3f4f6;
    }
    header {
      grid-column: 1 / -1;
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 6px 12px;
      border-bottom: 1px solid #d1d5db;
      background: #fff;
    }
    header h1 {
      margin: 0;
      font-size: 15px;
      font-weight: 650;
      letter-spacing: 0.01em;
    }
    header .dim {
      color: #9ca3af;
      font-size: 12px;
      white-space: nowrap;
    }
    header .status {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: #6b7280;
    }
    header button,
    header select,
    header label.button {
      font: inherit;
      font-size: 12px;
      padding: 3px 9px;
      border: 1px solid #d1d5db;
      border-radius: 5px;
      background: #fff;
      color: #111827;
      cursor: pointer;
    }
    header input[type='file'] {
      display: none;
    }
    /* Every pane is placed explicitly, since row-locked items would otherwise go first. */
    texelscope-source-view {
      grid-column: 1;
      grid-row: 2 / 4;
      border-right: 1px solid #d1d5db;
      min-height: 0;
    }
    texelscope-render-view {
      grid-column: 2;
      grid-row: 2;
      min-height: 0;
    }
    texelscope-region-view {
      grid-column: 2;
      grid-row: 3;
      border-top: 1px solid #d1d5db;
      min-height: 0;
    }
    texelscope-inspector {
      grid-column: 3;
      grid-row: 2 / 4;
      border-left: 1px solid #d1d5db;
      min-height: 0;
    }
    .drop {
      position: fixed;
      inset: 0;
      display: grid;
      place-items: center;
      background: rgba(37, 99, 235, 0.12);
      border: 3px dashed #2563eb;
      font-size: 18px;
      color: #1e3a8a;
      pointer-events: none;
    }
  `;

  declare frames: Frame[];
  declare current: number;
  declare status: string;
  declare description: Description | null;
  declare source: string;
  declare sourceError: string;
  declare pixel: [number, number] | null;
  declare explanation: Explanation | null;
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
  declare gpuError: string;
  declare dragging: boolean;

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
    this.gpuError = '';
    this.dragging = false;
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
  }

  override disconnectedCallback() {
    window.removeEventListener('message', this.#onMessage);
    window.removeEventListener('keydown', this.#onKey);
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
        <h1>Texelscope</h1>
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
      </header>
      <texelscope-source-view
        .source=${this.source}
        .highlight=${this.highlight}
        .values=${this.#inlineValues()}
        .error=${this.sourceError}
        @edit=${(e: CustomEvent<string>) => this.#edit(e.detail)}
      ></texelscope-source-view>
      <texelscope-render-view
        .targetWidth=${target?.width ?? 0}
        .targetHeight=${target?.height ?? 0}
        .selected=${this.pixel}
        .region=${this.regionRect}
        .overlay=${this.#overlay()}
        .message=${frame ? this.gpuError : 'No capture yet.\nUse Example to try one, or send one from your app.'}
        @pick=${(e: CustomEvent<[number, number]>) => this.#pick(e.detail)}
        @region=${(e: CustomEvent<Rect>) => this.#inspectRegion(e.detail)}
      ></texelscope-render-view>
      <texelscope-region-view
        .region=${this.region}
        .busy=${this.regionBusy}
        .error=${this.regionError}
        .shown=${this.shown}
        .source=${this.#valid}
        @show=${(e: CustomEvent<string>) => (this.shown = e.detail)}
        @highlight=${(e: CustomEvent<SourceSpan>) => (this.highlight = e.detail)}
      ></texelscope-region-view>
      <texelscope-inspector
        .explanation=${this.explanation}
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
      ${this.dragging ? html`<div class="drop">Drop a capture to open it</div>` : nothing}
    `;
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
    await this.#renderGpu();
    if (pixel) await this.#pick(pixel);
    if (this.regionRect) await this.#inspectRegion(this.regionRect);
  }

  async #renderGpu() {
    if (!this.#replay || !this.description) return;
    const overrides = this.whatIf.map((w) => w.override);
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
    this.explanation = null;
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
  #inlineValues(): InlineValue[] {
    const trace = this.trace;
    if (!trace) return [];
    const source = this.#valid;
    const lineOf = (offset: number) => {
      let line = 1;
      for (let i = 0; i < offset && i < source.length; i++) if (source.charCodeAt(i) === 10) line++;
      return line;
    };
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
