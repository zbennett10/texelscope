import type { Capture } from './capture.js';

/** Where `npm run dev` serves the Texelscope UI. */
export const DEFAULT_UI_URL = 'http://localhost:5173/';

/** The window name the UI tab gets, so a reloaded app finds the same tab again. */
const WINDOW_NAME = 'texelscope';

/** Where the page's UI is, shared by every copy of the SDK on the page through the global registry. */
const PAGE_UI = Symbol.for('texelscope.ui');
type PageScope = { [PAGE_UI]?: () => string | undefined };

/**
 * Sends captures that name no UI to this one, from every copy of the SDK on the page. The Vite
 * plugin and the extension call it, so an app's own calls reach the UI they serve.
 */
export function setDefaultUi(resolve: () => string | undefined): void {
  (globalThis as PageScope)[PAGE_UI] = resolve;
}

/** The UI to use when a call names none. */
function defaultUi(): string {
  return (globalThis as PageScope)[PAGE_UI]?.() ?? DEFAULT_UI_URL;
}

export interface OpenOptions {
  /** The Texelscope UI to open. Defaults to the one the Vite plugin or extension serves, else the dev server's. */
  url?: string;
  /** A pixel to select when the capture opens. */
  pixel?: [number, number];
  /** Brings the Texelscope tab to the front. On by default, and worth turning off when streaming. */
  focus?: boolean;
  /** How long to wait for the UI to answer, in milliseconds. */
  timeout?: number;
}

/** The UI tab, and the origin it answers from once it is listening. */
let ui: { window: Window; ready: Promise<string> } | undefined;

/**
 * Opens the Texelscope UI in another tab, or reuses the one already open, and sends it a
 * capture. Call it from a page, since workers can't open windows.
 */
export async function openInTexelscope(capture: Capture, options: OpenOptions = {}): Promise<void> {
  const url = options.url ?? defaultUi();
  connectTexelscope(url);
  if (!ui) throw new Error(`the browser blocked opening ${url}`);
  const timeout = options.timeout ?? 20000;
  let origin: string;
  try {
    // A tab found by name may hold the UI from another address, such as the dev server's.
    origin = await within(ui.ready, timeout);
  } catch {
    ui = undefined;
    throw new Error(`Texelscope did not answer at ${url} within ${timeout / 1000} s. Is its UI running?`);
  }
  ui.window.postMessage({ type: 'texelscope:capture', capture, pixel: options.pixel }, origin);
  if (options.focus !== false) ui.window.focus();
}

/** Whether a Texelscope tab this page opened is still open to send to. */
export function isTexelscopeOpen(): boolean {
  return ui !== undefined && !ui.window.closed;
}

/**
 * Opens the Texelscope UI, or finds the tab already open, without sending anything yet. Call
 * it while handling a click: browsers only let a page open a window during a user gesture,
 * and a capture can take longer than that gesture lasts.
 */
export function connectTexelscope(url: string = defaultUi()): void {
  if (ui && !ui.window.closed) return;
  // An empty URL finds a tab this page opened before it reloaded, without reloading it.
  const target = window.open('', WINDOW_NAME);
  if (!target) return;
  const blank = isBlank(target);
  if (blank) target.location.href = url;
  ui = { window: target, ready: readyFrom(target, !blank) };
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

/**
 * Waits for the UI to say it is listening, asking it first when it is already loaded, and
 * resolves with the origin it answered from. The hello carries nothing, so it goes to any origin.
 */
function readyFrom(target: Window, loaded: boolean): Promise<string> {
  return new Promise((resolve) => {
    const listen = (event: MessageEvent) => {
      if (event.source === target && event.data?.type === 'texelscope:ready') {
        window.removeEventListener('message', listen);
        resolve(event.origin);
      }
    };
    window.addEventListener('message', listen);
    if (loaded) target.postMessage({ type: 'texelscope:hello' }, '*');
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
