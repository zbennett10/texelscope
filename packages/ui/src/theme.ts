// Texelscope's colors, as custom properties that every pane inherits from the app. Dark is the
// default, and the `theme="light"` attribute on the app switches to light.

import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorView } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { css } from 'lit';

export const themeTokens = css`
  :host {
    color-scheme: dark;
    --ts-sans: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
    --ts-mono: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;

    --ts-bg: #0a0c10;
    --ts-header: #0c0e12;
    --ts-surface: #0e1116;
    --ts-surface-2: #111419;
    --ts-raised: #171b22;
    --ts-raised-hover: #1f242d;
    --ts-hover: rgba(255, 255, 255, 0.04);
    --ts-border: #1d2129;
    --ts-border-strong: #2a303a;
    --ts-border-hover: #3c4350;
    --ts-text: #e3e6eb;
    --ts-text-2: #a9b0bc;
    --ts-text-3: #7d8595;
    --ts-text-4: #596070;
    --ts-focus: rgba(244, 114, 182, 0.6);

    --ts-accent: #f472b6;
    --ts-accent-soft: rgba(244, 114, 182, 0.08);
    --ts-accent-hover: rgba(244, 114, 182, 0.15);
    --ts-accent-text: #f9a8d4;
    --ts-blue: #60a5fa;
    --ts-blue-soft: rgba(96, 165, 250, 0.09);
    --ts-compare: rgba(96, 165, 250, 0.03);
    --ts-amber: #fbbf24;
    --ts-amber-text: #fcd34d;
    --ts-green-soft: rgba(74, 222, 128, 0.12);
    --ts-green-text: #86efac;
    --ts-red-soft: rgba(248, 113, 113, 0.1);
    --ts-red-text: #fca5a5;
    --ts-red-border: rgba(248, 113, 113, 0.28);

    --ts-badge: rgba(148, 163, 184, 0.13);
    --ts-badge-text: #b6bdc9;
    --ts-input: rgba(129, 140, 248, 0.15);
    --ts-input-text: #a5b4fc;
    --ts-texture: rgba(192, 132, 252, 0.15);
    --ts-texture-text: #d8b4fe;
    --ts-let: rgba(45, 212, 191, 0.13);
    --ts-let-text: #5eead4;
    --ts-var: rgba(251, 191, 36, 0.13);
    --ts-var-text: #fcd34d;
    --ts-code-chip: rgba(255, 255, 255, 0.07);

    --ts-checker-a: #0f1216;
    --ts-checker-b: #13171c;
    --ts-swatch-a: #3a404b;
    --ts-swatch-b: #252a33;
    --ts-swatch-border: rgba(255, 255, 255, 0.12);
    --ts-glass: rgba(15, 18, 23, 0.82);
    --ts-glass-border: rgba(255, 255, 255, 0.08);
    --ts-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    --ts-ch-r: #f87171;
    --ts-ch-g: #4ade80;
    --ts-ch-b: #60a5fa;
    --ts-ch-a: #9ca3af;

    --ts-code-bg: #0e1116;
    --ts-gutter: #3e4554;
    --ts-gutter-active: #a9b0bc;
    --ts-active-line: rgba(255, 255, 255, 0.025);
    --ts-selection: rgba(96, 165, 250, 0.24);
    --ts-selection-match: rgba(96, 165, 250, 0.12);
    --ts-cursor: #f472b6;
    --ts-inline: rgba(129, 140, 248, 0.11);
    --ts-inline-text: #aab6fb;
    --ts-mark: rgba(244, 114, 182, 0.2);
    --ts-mark-line: rgba(244, 114, 182, 0.06);
    --ts-syntax-keyword: #c3a6ff;
    --ts-syntax-type: #6fd6e8;
    --ts-syntax-function: #87b3ff;
    --ts-syntax-number: #f6ad85;
    --ts-syntax-comment: #5d6577;
    --ts-syntax-meta: #e5c07b;
    --ts-syntax-variable: #d6dbe4;
    --ts-syntax-special: #ff8fb1;
  }

  :host([theme='light']) {
    color-scheme: light;

    --ts-bg: #f3f4f6;
    --ts-header: #ffffff;
    --ts-surface: #ffffff;
    --ts-surface-2: #fafafa;
    --ts-raised: #ffffff;
    --ts-raised-hover: #f3f4f6;
    --ts-hover: #f3f4f6;
    --ts-border: #e5e7eb;
    --ts-border-strong: #d1d5db;
    --ts-border-hover: #9ca3af;
    --ts-text: #111827;
    --ts-text-2: #4b5563;
    --ts-text-3: #6b7280;
    --ts-text-4: #9ca3af;
    --ts-focus: rgba(219, 39, 119, 0.45);

    --ts-accent: #db2777;
    --ts-accent-soft: #fdf2f8;
    --ts-accent-hover: #fce7f3;
    --ts-accent-text: #9d174d;
    --ts-blue: #2563eb;
    --ts-blue-soft: #eff6ff;
    --ts-compare: #f8fafc;
    --ts-amber: #d97706;
    --ts-amber-text: #b45309;
    --ts-green-soft: #dcfce7;
    --ts-green-text: #166534;
    --ts-red-soft: #fef2f2;
    --ts-red-text: #991b1b;
    --ts-red-border: #fecaca;

    --ts-badge: #e5e7eb;
    --ts-badge-text: #374151;
    --ts-input: #e0e7ff;
    --ts-input-text: #3730a3;
    --ts-texture: #f3e8ff;
    --ts-texture-text: #6b21a8;
    --ts-let: #ccfbf1;
    --ts-let-text: #115e59;
    --ts-var: #fef3c7;
    --ts-var-text: #92400e;
    --ts-code-chip: rgba(17, 24, 39, 0.06);

    --ts-checker-a: #e4e6ea;
    --ts-checker-b: #f2f3f5;
    --ts-swatch-a: #dddddd;
    --ts-swatch-b: #ffffff;
    --ts-swatch-border: rgba(0, 0, 0, 0.15);
    --ts-glass: rgba(255, 255, 255, 0.94);
    --ts-glass-border: rgba(17, 24, 39, 0.08);
    --ts-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
    --ts-ch-r: #dc2626;
    --ts-ch-g: #16a34a;
    --ts-ch-b: #2563eb;
    --ts-ch-a: #6b7280;

    --ts-code-bg: #ffffff;
    --ts-gutter: #9ca3af;
    --ts-gutter-active: #374151;
    --ts-active-line: rgba(17, 24, 39, 0.03);
    --ts-selection: #dbeafe;
    --ts-selection-match: rgba(37, 99, 235, 0.1);
    --ts-cursor: #111827;
    --ts-inline: #eef2ff;
    --ts-inline-text: #3730a3;
    --ts-mark: rgba(219, 39, 119, 0.16);
    --ts-mark-line: rgba(219, 39, 119, 0.05);
    --ts-syntax-keyword: #8250df;
    --ts-syntax-type: #0a7f6f;
    --ts-syntax-function: #0550ae;
    --ts-syntax-number: #b35900;
    --ts-syntax-comment: #8b949e;
    --ts-syntax-meta: #9a6700;
    --ts-syntax-variable: #1f2328;
    --ts-syntax-special: #cf222e;
  }
`;

/** The editor's look, and Texelscope's marks in it, drawn from the custom properties. */
export const editorTheme = EditorView.theme({
  '&': { height: '100%', color: 'var(--ts-syntax-variable)', backgroundColor: 'var(--ts-code-bg)', fontSize: '12.5px' },
  '.cm-scroller': { fontFamily: 'var(--ts-mono)', lineHeight: '1.6' },
  '.cm-content': { caretColor: 'var(--ts-cursor)', padding: '6px 0' },
  '.cm-cursor, .cm-dropCursor': { borderLeft: '2px solid var(--ts-cursor)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
    backgroundColor: 'var(--ts-selection)',
  },
  '.cm-gutters': { backgroundColor: 'var(--ts-code-bg)', color: 'var(--ts-gutter)', border: 'none' },
  '.cm-lineNumbers .cm-gutterElement': { padding: '0 8px 0 14px' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent', color: 'var(--ts-gutter-active)' },
  '.cm-activeLine': { backgroundColor: 'var(--ts-active-line)' },
  '.cm-selectionMatch': { backgroundColor: 'var(--ts-selection-match)' },
  '&.cm-focused .cm-matchingBracket': { backgroundColor: 'var(--ts-selection-match)', outline: '1px solid var(--ts-border-hover)' },
  '&.cm-focused .cm-nonmatchingBracket': { backgroundColor: 'var(--ts-red-soft)' },
  '.cm-foldPlaceholder': { backgroundColor: 'var(--ts-raised)', border: '1px solid var(--ts-border-strong)', color: 'var(--ts-text-3)' },
  '.cm-tooltip': {
    backgroundColor: 'var(--ts-surface-2)',
    border: '1px solid var(--ts-border-strong)',
    borderRadius: '6px',
    color: 'var(--ts-text)',
    boxShadow: 'var(--ts-shadow)',
  },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--ts-accent-hover)', color: 'var(--ts-text)' },
  '.cm-panels': { backgroundColor: 'var(--ts-surface-2)', color: 'var(--ts-text)' },
  '.cm-panels.cm-panels-top': { borderBottom: '1px solid var(--ts-border)' },
  '.cm-panels.cm-panels-bottom': { borderTop: '1px solid var(--ts-border)' },
  '.cm-textfield': {
    backgroundColor: 'var(--ts-raised)',
    border: '1px solid var(--ts-border-strong)',
    borderRadius: '4px',
    color: 'var(--ts-text)',
  },
  '.cm-button': {
    backgroundImage: 'none',
    backgroundColor: 'var(--ts-raised)',
    border: '1px solid var(--ts-border-strong)',
    borderRadius: '4px',
    color: 'var(--ts-text)',
  },
  '.cm-searchMatch': { backgroundColor: 'rgba(251, 191, 36, 0.2)', outline: '1px solid rgba(251, 191, 36, 0.45)' },
  '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: 'rgba(251, 191, 36, 0.4)' },
  // Texelscope's marks come last, so a marked line shows over the active line.
  '.ts-highlight': { backgroundColor: 'var(--ts-mark)', borderBottom: '2px solid var(--ts-accent)' },
  '.ts-line': { backgroundColor: 'var(--ts-mark-line)' },
  '.ts-not-run': { opacity: '0.42' },
  '.ts-inline': {
    marginLeft: '1.5em',
    padding: '1px 7px',
    borderRadius: '4px',
    backgroundColor: 'var(--ts-inline)',
    color: 'var(--ts-inline-text)',
    fontSize: '11.5px',
  },
});

/** Syntax colors for the WGSL and GLSL highlighters, also from the custom properties. */
export const syntaxColors = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.keyword, color: 'var(--ts-syntax-keyword)' },
    { tag: tags.typeName, color: 'var(--ts-syntax-type)' },
    { tag: tags.function(tags.variableName), color: 'var(--ts-syntax-function)' },
    { tag: tags.special(tags.variableName), color: 'var(--ts-syntax-special)' },
    { tag: [tags.number, tags.atom], color: 'var(--ts-syntax-number)' },
    { tag: tags.comment, color: 'var(--ts-syntax-comment)', fontStyle: 'italic' },
    { tag: tags.meta, color: 'var(--ts-syntax-meta)' },
    { tag: tags.variableName, color: 'var(--ts-syntax-variable)' },
  ]),
);
