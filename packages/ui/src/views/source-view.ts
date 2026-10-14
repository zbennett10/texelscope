import { LitElement, css, html } from 'lit';
import { Compartment, EditorState, RangeSetBuilder, StateEffect, StateField } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { basicSetup } from 'codemirror';
import { glsl } from '../glsl-language.js';
import type { Theme } from '../prefs.js';
import { editorTheme, syntaxColors } from '../theme.js';
import type { SourceSpan } from '../types.js';
import { wgsl } from '../wgsl-language.js';

/** A value shown after the end of a source line. */
export interface InlineValue {
  line: number;
  text: string;
}

class ValueWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  override eq(other: ValueWidget) {
    return other.text === this.text;
  }
  toDOM() {
    const span = document.createElement('span');
    span.className = 'ts-inline';
    span.textContent = this.text;
    return span;
  }
}

const setMarks = StateEffect.define<{ highlight: SourceSpan | null; values: InlineValue[]; notRun: number[] }>();

const marks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(set, tr) {
    set = set.map(tr.changes);
    for (const effect of tr.effects) {
      if (!effect.is(setMarks)) continue;
      const doc = tr.state.doc;
      const ranges: { from: number; to: number; deco: Decoration }[] = [];
      const { highlight, values, notRun } = effect.value;
      for (const n of notRun) {
        if (n < 1 || n > doc.lines) continue;
        const from = doc.line(n).from;
        ranges.push({ from, to: from, deco: Decoration.line({ class: 'ts-not-run' }) });
      }
      if (highlight && highlight.end <= doc.length && highlight.start < highlight.end) {
        ranges.push({ from: highlight.start, to: highlight.end, deco: Decoration.mark({ class: 'ts-highlight' }) });
        const line = doc.lineAt(highlight.start);
        ranges.push({ from: line.from, to: line.from, deco: Decoration.line({ class: 'ts-line' }) });
      }
      for (const value of values) {
        if (value.line < 1 || value.line > doc.lines) continue;
        const end = doc.line(value.line).to;
        ranges.push({ from: end, to: end, deco: Decoration.widget({ widget: new ValueWidget(value.text), side: 1 }) });
      }
      ranges.sort((a, b) => a.from - b.from || a.to - b.to);
      const builder = new RangeSetBuilder<Decoration>();
      for (const r of ranges) builder.add(r.from, r.to, r.deco);
      set = builder.finish();
    }
    return set;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** Editable shader source. Edits arrive as debounced `edit` events. */
export class TexelscopeSourceView extends LitElement {
  static override properties = {
    source: { type: String },
    highlight: { attribute: false },
    values: { attribute: false },
    notRun: { attribute: false },
    language: { type: String },
    theme: { type: String },
    error: { type: String },
  };

  static override styles = css`
    :host {
      display: grid;
      grid-template-rows: minmax(0, 1fr) auto;
      min-height: 0;
      background: var(--ts-code-bg);
    }
    .editor {
      min-height: 0;
      overflow: hidden;
    }
    .error {
      max-height: 30%;
      overflow: auto;
      margin: 0;
      padding: 8px 12px;
      border-top: 1px solid var(--ts-red-border);
      background: var(--ts-red-soft);
      color: var(--ts-red-text);
      font: 11.5px/1.5 var(--ts-mono);
      white-space: pre-wrap;
    }
  `;

  declare source: string;
  declare highlight: SourceSpan | null;
  declare values: InlineValue[];
  /** Lines of code the selected pixel didn't run, counted from 1, shown dimmed. */
  declare notRun: number[];
  /** The source's language, for highlighting. */
  declare language: 'wgsl' | 'glsl';
  /** Picks CodeMirror's own light or dark styles, for the parts the editor theme leaves alone. */
  declare theme: Theme;
  declare error: string;
  #highlighting = new Compartment();
  #scheme = new Compartment();

  #view?: EditorView;
  #timer?: number;
  /** Set while the source is replaced from outside, so the change is not reported back. */
  #external = false;

  constructor() {
    super();
    this.source = '';
    this.highlight = null;
    this.values = [];
    this.notRun = [];
    this.language = 'wgsl';
    this.theme = 'dark';
    this.error = '';
  }

  override disconnectedCallback() {
    this.#view?.destroy();
    this.#view = undefined;
    super.disconnectedCallback();
  }

  override render() {
    return html`<div class="editor"></div>${this.error ? html`<pre class="error">${this.error}</pre>` : ''}`;
  }

  override updated(changed: Map<string, unknown>) {
    if (!this.#view) {
      const parent = this.renderRoot.querySelector('.editor');
      if (!parent || !this.shadowRoot) return;
      this.#view = new EditorView({
        parent,
        root: this.shadowRoot,
        state: EditorState.create({
          doc: this.source,
          extensions: [
            basicSetup,
            editorTheme,
            syntaxColors,
            this.#scheme.of(EditorView.darkTheme.of(this.theme === 'dark')),
            this.#highlighting.of(this.language === 'glsl' ? glsl : wgsl),
            marks,
            EditorView.updateListener.of((update) => {
              if (update.docChanged && !this.#external) this.#changed(update.state.doc.toString());
            }),
          ],
        }),
      });
    } else {
      if (changed.has('language')) this.#view.dispatch({ effects: this.#highlighting.reconfigure(this.language === 'glsl' ? glsl : wgsl) });
      if (changed.has('theme')) this.#view.dispatch({ effects: this.#scheme.reconfigure(EditorView.darkTheme.of(this.theme === 'dark')) });
    }
    if (changed.has('source') && this.#view.state.doc.toString() !== this.source) {
      this.#external = true;
      this.#view.dispatch({ changes: { from: 0, to: this.#view.state.doc.length, insert: this.source } });
      this.#external = false;
    }
    if (changed.has('highlight') || changed.has('values') || changed.has('notRun') || changed.has('source')) {
      this.#view.dispatch({ effects: setMarks.of({ highlight: this.highlight, values: this.values, notRun: this.notRun }) });
      if (changed.has('highlight') && this.highlight) {
        const at = Math.min(this.highlight.start, this.#view.state.doc.length);
        this.#view.dispatch({ effects: EditorView.scrollIntoView(at, { y: 'center' }) });
      }
    }
  }

  #changed(text: string) {
    clearTimeout(this.#timer);
    this.#timer = window.setTimeout(() => {
      this.dispatchEvent(new CustomEvent('edit', { detail: text }));
    }, 450);
  }
}

customElements.define('texelscope-source-view', TexelscopeSourceView);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-source-view': TexelscopeSourceView;
  }
}
