import type { Capture } from './capture.js';

/** Where `npm run dev` serves the Texelscope UI. */
export const DEFAULT_UI_URL = 'http://localhost:5173/';

export interface OpenOptions {
  /** The Texelscope UI to open. */
  url?: string;
  /** A pixel to select when the capture opens. */
  pixel?: [number, number];
  /** Brings the Texelscope tab to the front. On by default, and worth turning off when streaming. */
  focus?: boolean;
}

let ui: { window: Window; ready: Promise<void> } | undefined;

/**
 * Opens the Texelscope UI in another tab, or reuses the one already open, and sends it a
 * capture. Call it from a page, since workers can't open windows.
 */
export async function openInTexelscope(capture: Capture, options: OpenOptions = {}): Promise<void> {
  const url = options.url ?? DEFAULT_UI_URL;
  if (!ui || ui.window.closed) {
    const opened = window.open(url, 'texelscope');
    if (!opened) throw new Error(`the browser blocked opening ${url}`);
    ui = { window: opened, ready: readyFrom(opened) };
  }
  await ui.ready;
  ui.window.postMessage({ type: 'texelscope:capture', capture, pixel: options.pixel }, new URL(url).origin);
  if (options.focus !== false) ui.window.focus();
}

/** Waits for the UI to say it is listening. */
function readyFrom(target: Window): Promise<void> {
  return new Promise((resolve) => {
    const listen = (event: MessageEvent) => {
      if (event.source === target && event.data?.type === 'texelscope:ready') {
        window.removeEventListener('message', listen);
        resolve();
      }
    };
    window.addEventListener('message', listen);
  });
}

/** Saves a capture as a JSON file the CLI and the UI can open. */
export function downloadCapture(capture: Capture, filename = 'capture.json'): void {
  const blob = new Blob([JSON.stringify(capture)], { type: 'application/json' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = filename;
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 0);
}
