import { LitElement, css, html, nothing } from 'lit';
import { css as colorCss, fixed, formatValue, numbers, parseLike } from '../format.js';
import { quantize } from '../gpu/replay.js';
import type { Comparison, Explanation, Link, Reason, SourceSpan, Step, Trace, Value } from '../types.js';
import { callTree, type LinkNode, type LinkPlace } from './link-tree.js';

/** How each kind of value is labeled, and what it means. */
const KIND: Record<Link['kind'], { label: string; title: string }> = {
  input: { label: 'input', title: 'A fragment input, such as the pixel position' },
  buffer: { label: 'buffer', title: 'Read from a uniform or storage buffer' },
  texture: { label: 'texture', title: 'Read from a texture' },
  let: { label: 'let', title: 'A named value the shader computed' },
  var: { label: 'var', title: 'The value a variable was last assigned' },
};

/** A value's name, without the texel a texture read names again on its detail line. */
function shortName(link: Link): string {
  return link.kind === 'texture' ? link.name.replace(/ \([^)]*\)$/, '') : link.name;
}

/** Why the selected pixel has its color: values, decisions, sensitivities and the trace. */
export class TexelscopeInspector extends LitElement {
  static override properties = {
    explanation: { attribute: false },
    comparison: { attribute: false },
    trace: { attribute: false },
    gpuColor: { attribute: false },
    gpuNote: { type: String },
    parity: { type: Boolean },
    format: { type: String },
    whatIf: { attribute: false },
    source: { type: String },
    language: { type: String },
    busy: { type: Boolean },
    error: { type: String },
    step: { attribute: false },
    namedOnly: { state: true },
    showSettings: { state: true },
    open: { state: true },
    closed: { state: true },
    editing: { state: true },
  };

  static override styles = css`
    :host {
      display: block;
      overflow: auto;
      background: var(--ts-surface-2);
      font: 12.5px/1.5 var(--ts-sans);
      color: var(--ts-text);
    }
    section {
      padding: 12px 14px;
      border-bottom: 1px solid var(--ts-border);
    }
    h2 {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 8px;
      margin: 0 0 8px;
      font-size: 10.5px;
      font-weight: 600;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--ts-text-3);
    }
    h2 button,
    h2 label {
      text-transform: none;
      letter-spacing: 0;
      font-weight: 400;
    }
    .why {
      margin: 10px 0 2px;
    }
    .why.versus p {
      border-left-color: var(--ts-blue);
      background: var(--ts-blue-soft);
    }
    .compare {
      background: var(--ts-compare);
    }
    .compare h3 {
      margin: 12px 0 4px;
      font-size: 11px;
      font-weight: 600;
      color: var(--ts-text-3);
    }
    .vs {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(0, auto) minmax(0, auto);
      gap: 2px 10px;
      align-items: baseline;
      padding: 2px 6px;
      border-radius: 5px;
      font: 11.5px var(--ts-mono);
    }
    .vs:hover {
      background: var(--ts-blue-soft);
    }
    .vs.same {
      color: var(--ts-text-4);
    }
    .vs.head {
      color: var(--ts-text-3);
      font: 600 10.5px var(--ts-sans);
      text-align: right;
    }
    .vs .v {
      max-width: 9em;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      text-align: right;
    }
    details summary {
      margin-top: 8px;
      color: var(--ts-text-2);
      cursor: pointer;
    }
    .why p {
      margin: 0 0 6px;
      padding: 6px 10px;
      border-left: 2px solid var(--ts-accent);
      border-radius: 0 6px 6px 0;
      background: var(--ts-accent-soft);
      font-size: 12.5px;
      line-height: 1.55;
      cursor: default;
    }
    .why code {
      padding: 1px 4px;
      border-radius: 4px;
      background: var(--ts-code-chip);
      font: 11.5px var(--ts-mono);
    }
    .intro {
      margin: -2px 0 8px;
      color: var(--ts-text-3);
      font-size: 11.5px;
      line-height: 1.5;
    }
    .scope {
      color: var(--ts-text-4);
    }
    .fold {
      display: block;
      width: 100%;
      margin: 1px 0;
      padding: 3px 6px;
      border: 0;
      border-radius: 5px;
      background: none;
      text-align: left;
      font: 600 11.5px var(--ts-mono);
      color: var(--ts-text-2);
    }
    .fold:hover {
      border-color: transparent;
      background: var(--ts-hover);
    }
    .fold .dim {
      margin-left: 6px;
      font-weight: 400;
    }
    .caret {
      display: inline-block;
      width: 12px;
      color: var(--ts-text-3);
    }
    .inside {
      margin-left: 9px;
      padding-left: 6px;
      border-left: 1px solid var(--ts-border);
    }
    .badge.input {
      background: var(--ts-input);
      color: var(--ts-input-text);
    }
    .badge.buffer {
      background: var(--ts-badge);
      color: var(--ts-badge-text);
    }
    .badge.texture {
      background: var(--ts-texture);
      color: var(--ts-texture-text);
    }
    .badge.let {
      background: var(--ts-let);
      color: var(--ts-let-text);
    }
    .badge.var {
      background: var(--ts-var);
      color: var(--ts-var-text);
    }
    .empty {
      padding: 28px 18px;
      color: var(--ts-text-3);
      line-height: 1.65;
    }
    .colors {
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 6px 10px;
      align-items: center;
    }
    .swatch {
      width: 34px;
      height: 20px;
      border-radius: 5px;
      border: 1px solid var(--ts-swatch-border);
      background-image: linear-gradient(var(--c), var(--c)), repeating-conic-gradient(var(--ts-swatch-a) 0 25%, var(--ts-swatch-b) 0 50%);
      background-size: auto, 8px 8px;
    }
    .mono {
      font-family: var(--ts-mono);
      font-size: 11.5px;
    }
    .dim {
      color: var(--ts-text-3);
    }
    .badge {
      display: inline-block;
      padding: 0 6px;
      border-radius: 4px;
      font-size: 10.5px;
      font-weight: 600;
      background: var(--ts-badge);
      color: var(--ts-badge-text);
    }
    .badge.ok {
      background: var(--ts-green-soft);
      color: var(--ts-green-text);
    }
    .badge.warn {
      background: var(--ts-red-soft);
      color: var(--ts-red-text);
    }
    .row {
      display: grid;
      grid-template-columns: 54px minmax(56px, 1fr) minmax(0, auto);
      gap: 2px 8px;
      align-items: baseline;
      padding: 3px 6px;
      border-radius: 5px;
      cursor: default;
    }
    .row:hover,
    .row.active {
      background: var(--ts-accent-soft);
    }
    .row .detail {
      grid-column: 2 / 4;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--ts-text-3);
      font-family: var(--ts-mono);
      font-size: 11px;
    }
    .name {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-family: var(--ts-mono);
      font-size: 11.5px;
    }
    .value {
      display: flex;
      align-items: baseline;
      justify-content: flex-end;
      gap: 6px;
      min-width: 0;
      font-family: var(--ts-mono);
      font-size: 11.5px;
    }
    .value .v {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    button {
      font: inherit;
      font-size: 11px;
      padding: 1px 8px;
      border: 1px solid var(--ts-border-strong);
      border-radius: 5px;
      background: var(--ts-raised);
      color: var(--ts-text-2);
      cursor: pointer;
      transition: background-color 0.15s, border-color 0.15s, color 0.15s;
    }
    button:hover {
      border-color: var(--ts-border-hover);
      background: var(--ts-raised-hover);
      color: var(--ts-text);
    }
    button:focus-visible {
      outline: 2px solid var(--ts-focus);
      outline-offset: 1px;
    }
    .decision {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .decision .detail {
      grid-column: 1 / 3;
      font-family: inherit;
    }
    .sense {
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
      border-radius: 1px;
      background: currentColor;
    }
    input.pin {
      width: 100%;
      box-sizing: border-box;
      font: 11.5px var(--ts-mono);
      padding: 2px 6px;
      border: 1px solid var(--ts-accent);
      border-radius: 4px;
      outline: none;
      background: var(--ts-raised);
      color: var(--ts-text);
      box-shadow: 0 0 0 3px var(--ts-accent-soft);
    }
    .steps {
      max-height: 340px;
      overflow: auto;
      border: 1px solid var(--ts-border);
      border-radius: 6px;
      background: var(--ts-surface);
    }
    .step {
      display: grid;
      grid-template-columns: 34px minmax(0, 1fr) auto;
      gap: 6px;
      padding: 1px 8px;
      font: 11px/1.65 var(--ts-mono);
      cursor: pointer;
    }
    .step:hover {
      background: var(--ts-hover);
    }
    .step.active {
      background: var(--ts-accent-hover);
    }
    .step .text {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .error {
      color: var(--ts-red-text);
      white-space: pre-wrap;
    }
    input[type='range'] {
      width: 100%;
    }
  `;

  declare explanation: Explanation | null;
  /** What sets the selected pixel apart from a second one, while comparing. */
  declare comparison: Comparison | null;
  declare trace: Trace | null;
  declare gpuColor: number[] | null;
  /** What the GPU color is, in a sentence. */
  declare gpuNote: string;
  /** Whether the GPU color is this draw's own, so it should match the CPU's. */
  declare parity: boolean;
  declare format: string;
  /** Labels of the overrides in effect. */
  declare whatIf: string[];
  declare source: string;
  /** The shader's language, which decides what can be pinned. */
  declare language: 'wgsl' | 'glsl';
  declare busy: boolean;
  declare error: string;
  declare step: number | null;
  declare namedOnly: boolean;
  /** Whether values that only read settings show in the list from data to color. */
  declare showSettings: boolean;
  /** Function calls opened in the list from data to color, by their tree keys. */
  declare open: Set<string>;
  /** Calls closed by hand that would otherwise start open. */
  declare closed: Set<string>;
  declare editing: number | null;

  constructor() {
    super();
    this.explanation = null;
    this.comparison = null;
    this.trace = null;
    this.gpuColor = null;
    this.gpuNote = 'GPU is what your app drew.';
    this.parity = true;
    this.format = 'rgba8unorm';
    this.whatIf = [];
    this.source = '';
    this.language = 'wgsl';
    this.busy = false;
    this.error = '';
    this.step = null;
    this.namedOnly = true;
    this.showSettings = false;
    this.open = new Set();
    this.closed = new Set();
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
      ${this.#comparison()} ${this.#colors(e)} ${this.#overrides()} ${this.#links(e)} ${this.#decisions(e)}
      ${this.#sensitivities(e)}
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
    let parity = this.parity ? html`<span class="badge">no GPU</span>` : nothing;
    if (gpu && cpu && this.parity) {
      // Compare with the CPU color as the target format would store it.
      const stored = quantize(this.format, cpu);
      const worst = Math.max(...stored.map((c, i) => Math.abs(c - (gpu[i] ?? 0))));
      const step = this.format.includes('8unorm') ? 1.5 / 255 : 1e-3;
      parity = worst <= step
        ? html`<span class="badge ok">GPU and CPU agree</span>`
        : html`<span class="badge warn">differ by ${fixed(worst)}</span>`;
    }
    const row = (label: string, rgba: number[] | null, missing: string) => html`
      <div class="swatch" style="--c: ${rgba ? colorCss(rgba) : 'transparent'}"></div>
      <div class="mono">${label} ${rgba ? rgba.map((c) => fixed(c, 3)).join(', ') : missing}</div>
    `;
    return html`<section>
      <h2>Pixel ${e.pixel[0]}, ${e.pixel[1]} ${parity}</h2>
      <div class="colors">${row('GPU', gpu, 'not drawn here')} ${row('CPU', cpu, 'discarded')}</div>
      ${this.#why(e)}
      <p class="intro" style="margin-top: 6px">
        ${this.gpuNote} CPU is Texelscope running the shader itself, which the rest of this panel explains. Code
        this pixel didn't run is dimmed in the source.
      </p>
      ${e.notes.map((n) => html`<div class="dim">${n}</div>`)}
    </section>`;
  }

  /** Why the pixel has its color, a few sentences long. */
  #why(e: Explanation) {
    return this.#reasons(e.why);
  }

  #reasons(reasons: Reason[], kind = '') {
    if (reasons.length === 0) return nothing;
    return html`<div class="why ${kind}">
      ${reasons.map(
        (reason) => html`<p @mouseenter=${() => this.#highlight(reason.span)} @click=${() => this.#highlight(reason.span)}>
          ${reason.text.split('`').map((part, i) => (i % 2 === 1 ? html`<code>${part}</code>` : part))}
        </p>`,
      )}
    </div>`;
  }

  /** What sets the selected pixel, A, apart from the one Alt-clicked, B. */
  #comparison() {
    const c = this.comparison;
    if (!c) return nothing;
    const swatch = (value: Value) => {
      const rgba = numbers(value);
      return html`<span class="swatch" style="--c: ${rgba.length ? colorCss(rgba) : 'transparent'}"></span>`;
    };
    const side = (v: boolean | null) => (v === null ? html`<span class="dim">not run</span>` : html`<span class="badge ${v ? 'ok' : ''}">${v}</span>`);
    const value = (v: Value | null) => (v === null ? '-' : formatValue(v));
    return html`<section class="compare">
      <h2>Comparing A and B <button @click=${() => this.#emit('stop-compare', null)}>Stop</button></h2>
      <div class="colors">
        ${swatch(c.colorA)}<div class="mono">A ${c.a[0]}, ${c.a[1]}</div>
        ${swatch(c.colorB)}<div class="mono">B ${c.b[0]}, ${c.b[1]}</div>
      </div>
      ${this.#reasons(c.summary, 'versus')}
      ${c.decisions.length > 0
        ? html`<h3>Decisions that differ</h3>
            ${c.decisions.map(
              (d) => html`<div class="vs" @mouseenter=${() => this.#highlight(d.span)}>
                <span class="name" title=${d.condition}>${d.condition}</span>
                <span>A ${side(d.a)}</span><span>B ${side(d.b)}</span>
              </div>`,
            )}`
        : nothing}
      ${c.values.length > 0
        ? html`<h3>Values on the way to the color</h3>
            <div class="vs head"><span></span><span>A</span><span>B</span></div>
            ${c.values.map((v) => {
              const same = v.a !== null && v.b !== null && formatValue(v.a) === formatValue(v.b);
              const name = v.scope ? `${v.scope}: ${v.name}` : v.name;
              return html`<div class="vs ${same ? 'same' : ''}" @mouseenter=${() => this.#highlight(v.span)}>
                <span class="name" title=${name}>${name}</span>
                <span class="v" title=${value(v.a)}>${value(v.a)}</span><span class="v" title=${value(v.b)}>${value(v.b)}</span>
              </div>`;
            })}`
        : nothing}
      <details>
        <summary>Why B has its color</summary>
        ${this.#reasons(c.whyB)}
      </details>
    </section>`;
  }

  #overrides() {
    if (this.whatIf.length === 0) return nothing;
    return html`<section>
      <h2>What if <button @click=${() => this.#emit('clear-overrides', null)}>Clear</button></h2>
      <p class="intro">Changes you're trying. The image and everything below show their effect.</p>
      ${this.whatIf.map(
        (label, i) => html`<div class="row decision">
          <span class="name" title=${label}>${label}</span>
          <button @click=${() => this.#emit('remove-override', i)}>Remove</button>
        </div>`,
      )}
    </section>`;
  }

  #links(e: Explanation) {
    const settings = e.links.filter((l) => l.setting).length;
    const places: LinkPlace[] = e.links.flatMap((link, index) => {
      if (link.setting && !this.showSettings) return [];
      const step = link.step !== null ? this.trace?.steps[link.step] : undefined;
      return [{ index, depth: step?.depth ?? 0, call: step?.call ?? 0, scope: link.scope }];
    });
    return html`<section>
      <h2>From data to color ${this.#settingsToggle(settings + e.decisions.filter((d) => d.setting).length)}</h2>
      <p class="intro">
        Every value this pixel computed on its way to its color, in order. Open a function call to see the values inside
        it. Pin a value to try another one.${settings > 0 && !this.showSettings ? ` ${settings} values that only read settings are hidden.` : ''}
      </p>
      ${this.#nodes(callTree(places), e, null)}
    </section>`;
  }

  /** The checkbox that shows values and decisions that only read settings. */
  #settingsToggle(count: number) {
    if (count === 0) return nothing;
    return html`<label class="dim" title="Values and decisions that come only from settings, the same at every pixel"
      ><input type="checkbox" .checked=${this.showSettings}
        @change=${(ev: Event) => (this.showSettings = (ev.target as HTMLInputElement).checked)} /> settings (${count})</label
    >`;
  }

  #nodes(nodes: LinkNode[], e: Explanation, within: string | null): unknown {
    // A lone call at the top, such as GLSL's main, holds the whole story, so it starts open.
    const lone = within === null && nodes.filter((n) => n.kind === 'call').length === 1;
    return nodes.map((node) => {
      if (node.kind === 'row') return this.#link(e.links[node.index]!, node.index, within);
      const open = this.open.has(node.key) || (lone && !this.closed.has(node.key));
      return html`<div class="call">
        <button class="fold" data-call=${node.key} aria-expanded=${open ? 'true' : 'false'} @click=${() => this.#toggle(node.key)}>
          <span class="caret">${open ? '▾' : '▸'}</span> ${node.name}
          <span class="dim">${node.size} values</span>
        </button>
        ${open ? html`<div class="inside">${this.#nodes(node.children, e, node.name)}</div>` : nothing}
      </div>`;
    });
  }

  #toggle(key: string) {
    const open = new Set(this.open);
    const closed = new Set(this.closed);
    const wasOpen = this.renderRoot.querySelector(`[data-call="${CSS.escape(key)}"]`)?.getAttribute('aria-expanded') === 'true';
    if (wasOpen) {
      open.delete(key);
      closed.add(key);
    } else {
      open.add(key);
      closed.delete(key);
    }
    this.open = open;
    this.closed = closed;
  }

  #link(link: Link, i: number, within: string | null) {
    const name = link.scope ? `${link.scope}: ${link.name}` : link.name;
    const numeric = !!link.expr && numbers(link.value).length > 0;
    // A var's value comes from its assignment, which a WGSL patch can't replace by itself yet.
    const pinnable = numeric && (link.kind !== 'var' || this.language === 'glsl');
    // Inside a call to its own function, a value needs no function name.
    const scope = link.scope && link.scope !== within ? html`<span class="scope">${link.scope}:</span> ` : nothing;
    return html`<div class="row" @mouseenter=${() => this.#highlight(link.span)} @click=${() => this.#highlight(link.span)}>
      <span class="badge ${link.kind}" title=${KIND[link.kind].title}>${KIND[link.kind].label}</span>
      <span class="name" title=${name}>${scope}${shortName(link)}</span>
      ${this.editing === i
        ? html`<input class="pin" autofocus .value=${numbers(link.value).map((n) => fixed(n, 6)).join(', ')}
            @keydown=${(ev: KeyboardEvent) => this.#pinKey(ev, link)} @blur=${() => (this.editing = null)} />`
        : html`<span class="value" @dblclick=${() => pinnable && (this.editing = i)}>
            <span class="v" title=${formatValue(link.value)}>${formatValue(link.value)}</span>
            ${pinnable ? html`<button title="Try another value" @click=${() => (this.editing = i)}>Pin</button>` : nothing}
            ${numeric && link.expr
              ? html`<button title="Show this value at every pixel, computed on the GPU"
                  @click=${() => this.#emit('probe', { expr: link.expr, name: link.name })}>Map</button>`
              : nothing}
          </span>`}
      <span class="detail" title=${link.detail}>${link.detail}</span>
    </div>`;
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
    const shown = e.decisions.filter((d) => this.showSettings || !d.setting);
    const hidden = e.decisions.length - shown.length;
    if (e.decisions.length === 0) return nothing;
    return html`<section>
      <h2>Decisions</h2>
      <p class="intro">
        Each test this pixel made, and how far its inputs were from going the other way. Flip one to see the other
        side.${hidden > 0 ? ` ${hidden} tests that only read settings are hidden.` : ''}
      </p>
      ${shown.map(
        (d) => html`<div class="row decision" @mouseenter=${() => this.#highlight(d.span)}>
          <span class="name" title=${d.condition}>${d.condition}</span>
          ${d.expr ? html`<button title="Try the other side" @click=${() => this.#emit('force', { condition: d.expr, taken: !d.taken, label: `${d.condition} forced ${!d.taken}` })}>Flip</button>` : html`<span></span>`}
          <span class="detail">
            <span class="badge ${d.taken ? 'ok' : ''}">${d.taken ? 'true' : 'false'}</span>
            ${d.margin !== null ? `${fixed(d.margin, 3)} from flipping` : ''}${d.count > 1 ? `, ran ${d.count} times` : ''}
            ${d.forced ? html`<span class="badge warn">forced</span>` : nothing}
          </span>
        </div>`,
      )}
    </section>`;
  }

  #sensitivities(e: Explanation) {
    if (e.sensitivities.length === 0) return nothing;
    const peak = Math.max(...e.sensitivities.flatMap((s) => s.dColor.map(Math.abs)), 1e-9);
    const channel = ['var(--ts-ch-r)', 'var(--ts-ch-g)', 'var(--ts-ch-b)', 'var(--ts-ch-a)'];
    return html`<section>
      <h2>Moves the color most</h2>
      <p class="intro">The inputs that change the color most when nudged. Bars show how much red, green, blue and alpha move.</p>
      ${e.sensitivities.map(
        (s) => html`<div class="row sense" title="Change in r, g, b, a per unit change of the input">
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
      <p class="intro">Every step the pixel ran. Pick one, or drag the slider, and the source shows values as of that step.</p>
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
