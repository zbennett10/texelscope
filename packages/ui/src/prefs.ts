// Settings kept between visits in this browser: the color theme and the pane widths.

import { parsePanes, type PaneWidths } from './panes.js';

export type Theme = 'dark' | 'light';

const THEME_KEY = 'texelscope.theme';
const PANES_KEY = 'texelscope.panes';

/** The theme saved as text. Dark unless light was chosen. */
export function parseTheme(text: string | undefined): Theme {
  return text === 'light' ? 'light' : 'dark';
}

export function loadTheme(): Theme {
  return parseTheme(read(THEME_KEY));
}

export function saveTheme(theme: Theme): void {
  write(THEME_KEY, theme);
}

/** The pane widths chosen by dragging. A side never dragged keeps its default. */
export function loadPanes(): Partial<PaneWidths> {
  return parsePanes(read(PANES_KEY));
}

export function savePanes(widths: Partial<PaneWidths>): void {
  const text = JSON.stringify(widths);
  write(PANES_KEY, text === '{}' ? undefined : text);
}

// Storage can be missing or blocked, in which case settings last until the window closes.
function read(key: string): string | undefined {
  try {
    return localStorage.getItem(key) ?? undefined;
  } catch {
    return undefined;
  }
}

function write(key: string, value: string | undefined): void {
  try {
    if (value === undefined) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Nothing to keep it in.
  }
}
