import { LitElement, css, html, nothing } from 'lit';
import { css as colorCss, fixed, formatValue, numbers, parseLike } from '../format.js';
import { quantize } from '../gpu/replay.js';
import type { Explanation, Link, SourceSpan, Step, Trace } from '../types.js';

const KIND_LABEL: Record<Link['kind'], string> = { input: 'in', buffer: 'uniform', texture: 'texture', let: 'let', var: 'var' };

/** Why the selected pixel has its color: values, decisions, sensitivities and the trace. */
export class TexelscopeInspector extends LitElement {
  static override properties = {
    explanation: { attribute: false },
    trace: { attribute: false },
    gpuColor: { attribute: false },
    format: { type: String },
    whatIf: { attribute: false },
    source: { type: String },
    busy: { type: Boolean },
    error: { type: String },
    step: { attribute: false },
    namedOnly: { state: true },
    editing: { state: true },
  };

  static override styles = css`
    :host {
      display: block;
      overflow: auto;
      background: #fafafa;
      font: 12.5px/1.45 system-ui, sans-serif;
      color: #111827;
    }
    section {
      padding: 10px 12px;
      border-bottom: 1px solid #e5e7eb;
    }
    h2 {
      display: flex;
      justify-content: space-between;
      align-items: baseline;
      margin: 0 0 6px;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: #6b7280;
    }
    h2 button {
      text-transform: none;
      letter-spacing: 0;
    }
    .empty {
      padding: 24px 16px;
      color: #6b7280;
      line-height: 1.6;
    }
    .colors {
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 4px 10px;
      align-items: center;
    }
    .swatch {
      width: 34px;
      height: 20px;
      border-radius: 4px;
      border: 1px solid rgba(0, 0, 0, 0.15);
      background-image: linear-gradient(var(--c), var(--c)), repeating-conic-gradient(#ddd 0 25%, #fff 0 50%);
      background-size: auto, 8px 8px;
    }
    .mono {
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11.5px;
    }
    .dim {
      color: #6b7280;
    }
    .badge {
      display: inline-block;
      padding: 0 5px;
      border-radius: 3px;
      font-size: 10.5px;
      font-weight: 600;
      background: #e5e7eb;
      color: #374151;
    }
    .badge.ok {
      background: #dcfce7;
      color: #166534;
    }
    .badge.warn {
      background: #fee2e2;
      color: #991b1b;
    }
    .row {
      display: grid;
      grid-template-columns: 54px minmax(0, 1fr) auto;
      gap: 2px 8px;
      align-items: baseline;
      padding: 3px 4px;
      border-radius: 4px;
      cursor: default;
    }
    .row:hover,
    .row.active {
      background: #fdf2f8;
    }
    .row .detail {
      grid-column: 2 / 4;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: #6b7280;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11px;
    }
    .name {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11.5px;
    }
    .value {
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11.5px;
      text-align: right;
    }
    button {
      font: inherit;
      font-size: 11px;
      padding: 1px 7px;
      border: 1px solid #d1d5db;
      border-radius: 4px;
      background: #fff;
      color: #374151;
      cursor: pointer;
    }
    button:hover {
      border-color: #9ca3af;
    }
    .decision {
      grid-template-columns: minmax(0, 1fr) auto auto;
    }
    .bars {
      display: flex;
      gap: 2px;
      align-items: flex-end;
      height: 14px;
    }
    .bars span {
      width: 6px;
      background: currentColor;
    }
    input.pin {
      width: 100%;
      box-sizing: border-box;
      font: 11.5px ui-monospace, SFMono-Regular, Menlo, monospace;
      padding: 2px 4px;
      border: 1px solid #db2777;
      border-radius: 3px;
    }
    .steps {
      max-height: 340px;
      overflow: auto;
      border: 1px solid #e5e7eb;
      border-radius: 4px;
      background: #fff;
    }
    .step {
      display: grid;
      grid-template-columns: 34px minmax(0, 1fr) auto;
      gap: 6px;
      padding: 1px 6px;
      font: 11px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace;
      cursor: pointer;
    }
    .step:hover {
      background: #f3f4f6;
    }
    .step.active {
      background: #fce7f3;
    }
    .step .text {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .error {
      color: #991b1b;
      white-space: pre-wrap;
    }
    input[type='range'] {
      width: 100%;
    }
  `;

  declare explanation: Explanation | null;
  declare trace: Trace | null;
  declare gpuColor: number[] | null;
  declare format: string;
  /** Labels of the overrides in effect. */
  declare whatIf: string[];
  declare source: string;
  declare busy: boolean;
  declare error: string;
  declare step: number | null;
  declare namedOnly: boolean;
  declare editing: number | null;

  constructor() {
    super();
    this.explanation = null;
    this.trace = null;
    this.gpuColor = null;
    this.format = 'rgba8unorm';
    this.whatIf = [];
    this.source = '';
    this.busy = false;
    this.error = '';
    this.step = null;
    this.namedOnly = true;
    this.editing = null;
  }

  override render() {
    if (this.error) return html`<section><div class="error">${this.error}</div></section>`;
    const e = this.explanation;
    if (!e) {
      return html`<div class="empty">
        ${this.busy ? 'Working...' : html`Click a pixel to see why it has its color. Shift-drag to inspect a region.
        Edit the source to try a change, or pin a value and flip a decision below.`}
      </div>`;
    }
    return html`
      ${this.#colors(e)} ${this.#overrides()} ${this.#links(e)} ${this.#decisions(e)} ${this.#sensitivities(e)}
      ${this.#trace()}
    `;
  }

  #emit(name: string, detail: unknown) {
    this.dispatchEvent(new CustomEvent(name, { detail, bubbles: true, composed: true }));
  }

  #highlight(span: SourceSpan | null) {
    if (span) this.#emit('highlight', span);
  }

  #colors(e: Explanation) {
    const cpu = e.discarded ? null : numbers(e.color);
    const gpu = this.gpuColor;
    let parity = html`<span class="badge">no GPU</span>`;
    if (gpu && cpu) {
      // Compare with the CPU color as the target format would store it.
      const stored = quantize(this.format, cpu);
      const worst = Math.max(...stored.map((c, i) => Math.abs(c - (gpu[i] ?? 0))));
      const step = this.format.includes('8unorm') ? 1.5 / 255 : 1e-3;
      parity = worst <= step
        ? html`<span class="badge ok">GPU and CPU agree</span>`
        : html`<span class="badge warn">differ by ${fixed(worst)}</span>`;
    }
    const row = (label: string, rgba: number[] | null) => html`
      <div class="swatch" style="--c: ${rgba ? colorCss(rgba) : 'transparent'}"></div>
      <div class="mono">${label} ${rgba ? rgba.map((c) => fixed(c, 3)).join(', ') : 'discarded'}</div>
    `;
    return html`<section>
      <h2>Pixel ${e.pixel[0]}, ${e.pixel[1]} ${parity}</h2>
      <div class="colors">${row('GPU', gpu)} ${row('CPU', cpu)}</div>
      ${e.notes.map((n) => html`<div class="dim">${n}</div>`)}
    </section>`;
  }

  #overrides() {
    if (this.whatIf.length === 0) return nothing;
    return html`<section>
      <h2>What if <button @click=${() => this.#emit('clear-overrides', null)}>Clear</button></h2>
      ${this.whatIf.map(
        (label, i) => html`<div class="row decision">
          <span class="name" title=${label}>${label}</span>
          <span></span>
          <button @click=${() => this.#emit('remove-override', i)}>Remove</button>
        </div>`,
      )}
    </section>`;
  }

  #links(e: Explanation) {
    return html`<section>
      <h2>From data to color</h2>
      ${e.links.map((link, i) => {
        const name = link.scope ? `${link.scope}: ${link.name}` : link.name;
        const pinnable = link.expr && link.kind !== 'var' && numbers(link.value).length > 0;
        return html`<div class="row" @mouseenter=${() => this.#highlight(link.span)} @click=${() => this.#highlight(link.span)}>
          <span class="badge">${KIND_LABEL[link.kind]}</span>
          <span class="name" title=${name}>${name}</span>
          ${this.editing === i
            ? html`<input class="pin" autofocus .value=${numbers(link.value).map((n) => fixed(n, 6)).join(', ')}
                @keydown=${(ev: KeyboardEvent) => this.#pinKey(ev, link)} @blur=${() => (this.editing = null)} />`
            : html`<span class="value" title="Click to try another value" @dblclick=${() => pinnable && (this.editing = i)}>
                ${formatValue(link.value)} ${pinnable ? html`<button @click=${() => (this.editing = i)}>Pin</button>` : nothing}
              </span>`}
          <span class="detail" title=${link.detail}>${link.detail}</span>
        </div>`;
      })}
    </section>`;
  }

  #pinKey(ev: KeyboardEvent, link: Link) {
    if (ev.key === 'Escape') this.editing = null;
    if (ev.key !== 'Enter' || !link.expr) return;
    const value = parseLike(link.value, (ev.target as HTMLInputElement).value);
    if (!value) return;
    this.editing = null;
    this.#emit('pin', { expr: link.expr, value, label: `${link.name} = ${formatValue(value)}` });
  }

  #decisions(e: Explanation) {
    if (e.decisions.length === 0) return nothing;
    return html`<section>
      <h2>Decisions</h2>
      ${e.decisions.map(
        (d) => html`<div class="row decision" @mouseenter=${() => this.#highlight(d.span)}>
          <span class="name" title=${d.condition}>${d.condition}</span>
          <span class="mono">
            <span class="badge ${d.taken ? 'ok' : ''}">${d.taken ? 'true' : 'false'}</span>
            ${d.margin !== null ? html`<span class="dim">${fixed(d.margin, 3)} from flipping</span>` : nothing}
            ${d.count > 1 ? html`<span class="dim">x${d.count}</span>` : nothing}
            ${d.forced ? html`<span class="badge warn">forced</span>` : nothing}
          </span>
          ${d.expr ? html`<button title="Try the other side" @click=${() => this.#emit('force', { condition: d.expr, taken: !d.taken, label: `${d.condition} forced ${!d.taken}` })}>Flip</button>` : html`<span></span>`}
        </div>`,
      )}
    </section>`;
  }

  #sensitivities(e: Explanation) {
    if (e.sensitivities.length === 0) return nothing;
    const peak = Math.max(...e.sensitivities.flatMap((s) => s.dColor.map(Math.abs)), 1e-9);
    const channel = ['#dc2626', '#16a34a', '#2563eb', '#6b7280'];
    return html`<section>
      <h2>Moves the color most</h2>
      ${e.sensitivities.map(
        (s) => html`<div class="row decision" title="Change in r, g, b, a per unit change of the input">
          <span class="name">${s.input}</span>
          <span class="dim mono">${fixed(s.value, 3)}</span>
          <span class="bars">${s.dColor.map((d, i) => html`<span style="height: ${Math.max(1, (Math.abs(d) / peak) * 14)}px; color: ${channel[i]}"></span>`)}</span>
        </div>`,
      )}
    </section>`;
  }

  #trace() {
    const trace = this.trace;
    if (!trace) return nothing;
    const visible = trace.steps
      .map((step, index) => ({ step, index }))
      .filter(({ step }) => !this.namedOnly || step.name || step.origin);
    const current = this.step !== null ? trace.steps[this.step] : undefined;
    return html`<section>
      <h2>
        Trace, ${trace.steps.length} steps
        <label class="dim"><input type="checkbox" .checked=${!this.namedOnly} @change=${(ev: Event) => (this.namedOnly = !(ev.target as HTMLInputElement).checked)} /> every step</label>
      </h2>
      <input type="range" min="0" max=${Math.max(0, trace.steps.length - 1)} .value=${String(this.step ?? 0)}
        @input=${(ev: Event) => this.#emit('select-step', Number((ev.target as HTMLInputElement).value))} />
      ${current ? this.#stepDetail(trace, current) : nothing}
      <div class="steps">
        ${visible.map(
          ({ step, index }) => html`<div class="step ${index === this.step ? 'active' : ''}" @click=${() => this.#emit('select-step', index)}>
            <span class="dim">${this.#line(step.span)}</span>
            <span class="text">${'  '.repeat(step.depth)}${this.#label(step)}</span>
            <span>${formatValue(step.value)}</span>
          </div>`,
        )}
      </div>
      ${trace.truncated ? html`<div class="dim">Recording stopped at the step limit.</div>` : nothing}
    </section>`;
  }

  #stepDetail(trace: Trace, step: Step) {
    return html`<div class="mono" style="margin: 4px 0 8px">
      <div><b>${this.#label(step)}</b> = ${formatValue(step.value)}</div>
      ${step.note ? html`<div class="dim">${step.note}</div>` : nothing}
      ${step.inputs.length > 0
        ? html`<div class="dim">from ${step.inputs.map(
            (i) => html`<button style="margin: 2px" @click=${() => this.#emit('select-step', i)}>${this.#label(trace.steps[i]!)}</button>`,
          )}</div>`
        : nothing}
    </div>`;
  }

  #label(step: Step): string {
    const text = step.span ? this.source.slice(step.span.start, step.span.end).replace(/\s+/g, ' ') : '';
    if (step.name && step.name !== text) return `${step.name} = ${text}`;
    return step.name ?? text;
  }

  #line(span: SourceSpan | null): string {
    if (!span) return '';
    let line = 1;
    for (let i = 0; i < span.start && i < this.source.length; i++) if (this.source.charCodeAt(i) === 10) line++;
    return String(line);
  }
}

customElements.define('texelscope-inspector', TexelscopeInspector);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-inspector': TexelscopeInspector;
  }
}
