import { LitElement, css, html } from 'lit';
import fieldWgsl from '../../../examples/scalar-field/field.wgsl?raw';
import './source-view.js';

/** The Texelscope shell with shader source, render view and inspector side by side. */
export class TexelscopeApp extends LitElement {
  static override styles = css`
    :host {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 320px;
      grid-template-rows: auto minmax(0, 1fr);
      height: 100vh;
      font-family: system-ui, sans-serif;
      color: #1d1d1f;
      background: #f5f5f7;
    }
    header {
      grid-column: 1 / -1;
      padding: 8px 16px;
      font-weight: 600;
      border-bottom: 1px solid #d2d2d7;
    }
    section,
    texelscope-source-view {
      min-height: 0;
      border-right: 1px solid #d2d2d7;
    }
    section {
      display: grid;
      place-items: center;
      color: #6e6e73;
    }
  `;

  override render() {
    return html`
      <header>Texelscope</header>
      <texelscope-source-view .source=${fieldWgsl}></texelscope-source-view>
      <section>Render view</section>
      <section>Inspector</section>
    `;
  }
}

customElements.define('texelscope-app', TexelscopeApp);

declare global {
  interface HTMLElementTagNameMap {
    'texelscope-app': TexelscopeApp;
  }
}
