import type { Capture } from './capture.js';

/** Where `npm run dev` serves the Texelscope UI. */
export const DEFAULT_UI_URL = 'http://localhost:5173/';

/** The window name the UI gets, so a reloaded app finds the same window again. */
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
  /** Brings the Texelscope window to the front. On by default, and worth turning off when streaming. */
  focus?: boolean;
  /** How long to wait for the UI to answer, in milliseconds. */
  timeout?: number;
}

/** The UI's window, and the origin it answers from once it is listening. */
let ui: { window: Window; ready: Promise<string>; origin?: string } | undefined;

/** Draws this page can draw again for the captures it sent, which the UI then asks about. */
const live = new WeakMap<Capture, unknown>();

/** Notes what the UI may ask this page about a capture, sent along with it. */
export function markLive(capture: Capture, about: unknown): void {
  live.set(capture, about);
}

type Answer = (message: unknown, transfer?: Transferable[]) => void;
let answer: ((request: { type: string }, reply: Answer) => void) | undefined;
let answering = false;

/**
 * Answers what the Texelscope window this page opened asks, and nothing from any other
 * window or origin.
 */
export function answerTexelscope(handler: (request: { type: string }, reply: Answer) => void): void {
  answer = handler;
  if (answering || typeof window === 'undefined') return;
  answering = true;
  window.addEventListener('message', (event: MessageEvent) => {
    const from = ui;
    const data = event.data as { type?: unknown } | undefined;
    if (!from || event.source !== from.window || event.origin !== from.origin || typeof data?.type !== 'string') return;
    answer?.(data as { type: string }, (message, transfer = []) => from.window.postMessage(message, from.origin!, transfer));
  });
}

/** Sends a message to the Texelscope window, once it is listening. Returns whether it went. */
export function tellTexelscope(message: unknown, transfer: Transferable[] = []): boolean {
  if (!ui?.origin || ui.window.closed) return false;
  ui.window.postMessage(message, ui.origin, transfer);
  return true;
}

/**
 * Opens the Texelscope UI in a window of its own, or reuses the one already open, and sends
 * it a capture. Call it from a page, since workers can't open windows.
 */
export async function openInTexelscope(capture: Capture, options: OpenOptions = {}): Promise<void> {
  const url = options.url ?? defaultUi();
  connectTexelscope(url);
  if (!ui) throw new Error(`the browser blocked opening ${url}`);
  const timeout = options.timeout ?? 20000;
  let origin: string;
  try {
    // A window found by name may hold the UI from another address, such as the dev server's.
    origin = await within(ui.ready, timeout);
  } catch {
    ui = undefined;
    throw new Error(`Texelscope did not answer at ${url} within ${timeout / 1000} s. Is its UI running?`);
  }
  const about = live.get(capture);
  ui.window.postMessage({ type: 'texelscope:capture', capture, pixel: options.pixel, ...(about !== undefined && { live: about }) }, origin);
  if (options.focus !== false) ui.window.focus();
}

/** Whether a Texelscope window this page opened is still open to send to. */
export function isTexelscopeOpen(): boolean {
  return ui !== undefined && !ui.window.closed;
}

/**
 * Opens the Texelscope UI, or finds the window already open, without sending anything yet.
 * Call it while handling a click: browsers only let a page open a window during a user
 * gesture, and a capture can take longer than that gesture lasts.
 */
export function connectTexelscope(url: string = defaultUi()): void {
  if (ui && !ui.window.closed) return;
  // An empty URL finds a window this page opened before it reloaded, without reloading it.
  const target = window.open('', WINDOW_NAME, beside());
  if (!target) return;
  const blank = isBlank(target);
  if (blank) target.location.href = url;
  const opened: NonNullable<typeof ui> = { window: target, ready: readyFrom(target, !blank) };
  opened.ready.then((origin) => (opened.origin = origin));
  ui = opened;
}

/**
 * A window to one side of the screen. A tab in front would hide the app's, and a hidden tab
 * stops drawing, so edits the app runs would never show.
 */
function beside(): string {
  const screen = (globalThis as { screen?: Screen & { availLeft?: number; availTop?: number } }).screen;
  if (!screen?.availWidth) return 'popup';
  const width = Math.round(Math.min(1440, screen.availWidth * 0.62));
  const height = Math.round(Math.min(960, screen.availHeight * 0.92));
  const left = (screen.availLeft ?? 0) + screen.availWidth - width;
  const top = (screen.availTop ?? 0) + Math.round((screen.availHeight - height) / 2);
  return `popup,width=${width},height=${height},left=${left},top=${top}`;
}

/** Whether a window is still the blank page `window.open` makes, rather than the UI. */
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
