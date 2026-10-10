import { LitElement, css, html } from 'lit';
import { EditorState } from '@codemirror/state';
import { EditorView, basicSetup } from 'codemirror';

/** Read-only shader source with line numbers. */
export class TexelscopeSourceView extends LitElement {
  static override properties = { source: { type: String } };

  static override styles = css`
    :host {
      display: block;
      overflow: auto;
      background: #fff;
    }
    .cm-editor {
      height: 100%;
    }
  `;

  declare source: string;
  #view?: EditorView;

  constructor() {
    super();
    this.source = '';
  }

  override connectedCallback() {
    super.connectedCallback();
    // Rebuild the editor if the element was moved in the DOM.
    this.requestUpdate();
  }

  override disconnectedCallback() {
    this.#view?.destroy();
    this.#view = undefined;
    super.disconnectedCallback();
  }

  override render() {
    return html`<div class="editor"></div>`;
  }

  override updated() {
    if (!this.#view) {
      const parent = this.renderRoot.querySelector('.editor');
      if (!parent || !this.shadowRoot) return;
      // CodeMirror injects its styles into the root it is given.
      this.#view = new EditorView({
        parent,
        root: this.shadowRoot,
        state: EditorState.create({
          doc: this.source,
          extensions: [basicSetup, EditorState.readOnly.of(true)],
        }),
      });
    } else if (this.#view.state.doc.toString() !== this.source) {
      this.#view.dispatch({
        changes: { from: 0, to: this.#view.state.doc.length, insert: this.source },
      });
    }
  }
}

customElements.define('texelscope-source-view', TexelscopeSourceView);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-source-view': TexelscopeSourceView;
  }
}
