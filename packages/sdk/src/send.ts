import type { Capture } from './capture.js';

/** Where `npm run dev` serves the Texelscope UI. */
export const DEFAULT_UI_URL = 'http://localhost:5173/';

/** The window name the UI tab gets, so a reloaded app finds the same tab again. */
const WINDOW_NAME = 'texelscope';

export interface OpenOptions {
  /** The Texelscope UI to open. */
  url?: string;
  /** A pixel to select when the capture opens. */
  pixel?: [number, number];
  /** Brings the Texelscope tab to the front. On by default, and worth turning off when streaming. */
  focus?: boolean;
  /** How long to wait for the UI to answer, in milliseconds. */
  timeout?: number;
}

let ui: { window: Window; ready: Promise<void> } | undefined;

/**
 * Opens the Texelscope UI in another tab, or reuses the one already open, and sends it a
 * capture. Call it from a page, since workers can't open windows.
 */
export async function openInTexelscope(capture: Capture, options: OpenOptions = {}): Promise<void> {
  const url = options.url ?? DEFAULT_UI_URL;
  const origin = new URL(url).origin;
  if (!ui || ui.window.closed) {
    // An empty URL finds a tab this page opened before it reloaded, without reloading it.
    const target = window.open('', WINDOW_NAME);
    if (!target) throw new Error(`the browser blocked opening ${url}`);
    const blank = isBlank(target);
    if (blank) target.location.href = url;
    ui = { window: target, ready: readyFrom(target, blank ? undefined : origin) };
  }
  const timeout = options.timeout ?? 20000;
  try {
    await within(ui.ready, timeout);
  } catch {
    ui = undefined;
    throw new Error(`Texelscope did not answer at ${url} within ${timeout / 1000} s. Is its UI running?`);
  }
  ui.window.postMessage({ type: 'texelscope:capture', capture, pixel: options.pixel }, origin);
  if (options.focus !== false) ui.window.focus();
}

/** Whether a window is still the blank page `window.open` makes, rather than a UI tab. */
function isBlank(target: Window): boolean {
  try {
    return target.location.href === 'about:blank';
  } catch {
    // Another origin, so it is a page that has already loaded.
    return false;
  }
}

/** Waits for the UI to say it is listening, asking it first when it is already loaded. */
function readyFrom(target: Window, loadedAt?: string): Promise<void> {
  return new Promise((resolve) => {
    const listen = (event: MessageEvent) => {
      if (event.source === target && event.data?.type === 'texelscope:ready') {
        window.removeEventListener('message', listen);
        resolve();
      }
    };
    window.addEventListener('message', listen);
    if (loadedAt) target.postMessage({ type: 'texelscope:hello' }, loadedAt);
  });
}

function within<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('timed out')), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
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
