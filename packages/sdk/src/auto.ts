// Debugging with no setup in the app: installs the WebGL2 and WebGPU hooks before the app
// starts, then turns an Alt-click on any canvas into a capture of the draw behind that pixel,
// sent to Texelscope. The Texelscope browser extension and Vite plugin start it for you.

import { animation } from './animation.js';
import type { Capture } from './capture.js';
import { DEFAULT_UI_URL, isTexelscopeOpen, openInTexelscope, setDefaultUi } from './send.js';
import { toast } from './toast.js';
import { installWebGL2, type WebGL2Session } from './webgl2/index.js';
import { installWebGPU, type WebGPUSession } from './webgpu/index.js';

export interface AutoOptions {
  /** Where the Texelscope UI is, or how to find out when a pixel is picked. */
  ui?: string | (() => string | undefined);
  /** How many pixels on a side WebGL captures record inputs for. */
  window?: number;
}

/** How long to wait for the app to draw after a pick, including time to make it draw by hand. */
const WAIT_MS = 20000;

/** Marks the page as hooked, across every copy of this code, say the extension's and the Vite plugin's. */
const STARTED = Symbol.for('texelscope.auto');

/** Starts the hooks and the Alt-click picker. Call it before the app's own scripts run. */
export function startAuto(options: AutoOptions = {}): void {
  const page = globalThis as { [STARTED]?: boolean };
  if (page[STARTED]) return;
  page[STARTED] = true;
  if (options.ui) setDefaultUi(() => uiUrl(options));
  const contexts = trackContexts();
  const gl: WebGL2Session | undefined = typeof WebGL2RenderingContext !== 'undefined' ? installWebGL2({ window: options.window }) : undefined;
  const gpu: WebGPUSession | undefined = typeof GPUDevice !== 'undefined' ? installWebGPU() : undefined;

  // The capture comes first: opening Texelscope's tab would hide this one, and hidden tabs don't draw.
  const pick = async (canvas: HTMLCanvasElement, kind: 'webgl2' | 'webgpu', x: number, y: number) => {
    toast(`Capturing the draw behind pixel ${x}, ${y}...`);
    try {
      let pending: Promise<Capture[]>;
      if (kind === 'webgl2' && gl) pending = gl.captureAt(canvas, x, y, { timeout: WAIT_MS });
      else if (gpu) pending = within(gpu.captureNextDraw(), WAIT_MS, `The app drew nothing within ${WAIT_MS / 1000} s.`).then((c) => [c]);
      else throw new Error(`no ${kind} hook is installed`);
      void nudge(pending, x, y);
      await send(await pending, x, y);
    } catch (error) {
      toast((error as Error).message, { problem: true });
    }
  };

  /** Gets an app that draws only when something changes to draw its last frame again. */
  const nudge = async (pending: Promise<unknown>, x: number, y: number) => {
    let settled = false;
    pending.then(
      () => (settled = true),
      () => (settled = true),
    );
    const draws = animation.draws;
    const quiet = async () => {
      await animation.next();
      await animation.next();
      return !settled && animation.draws === draws;
    };
    if (!(await quiet())) return;
    if (animation.redraw() && !(await quiet())) return;
    toast(`Waiting for the app to draw pixel ${x}, ${y}. It draws only when something changes, so change something, like moving the view a little.`);
  };

  /** Sends the captures, with the last one, which left the pixel as it is, opening in the tab. */
  const send = async (captures: Capture[], x: number, y: number) => {
    // A page may only open a tab shortly after a click, so a slow capture asks for another.
    if (!isTexelscopeOpen() && navigator.userActivation?.isActive === false) {
      toast(`Captured pixel ${x}, ${y}.`, { action: { label: 'Open in Texelscope', run: () => void send(captures, x, y).catch((error: Error) => toast(error.message, { problem: true })) } });
      return;
    }
    const ui = uiUrl(options);
    for (const capture of captures.slice(0, -1)) await openInTexelscope(capture, { url: ui, focus: false });
    await openInTexelscope(captures[captures.length - 1]!, { url: ui, pixel: [x, y] });
    toast(captures.length > 1 ? `Sent the ${captures.length} draws that changed pixel ${x}, ${y}. The last one is open.` : `Sent pixel ${x}, ${y} to Texelscope.`);
  };

  let swallowClick = false;
  addEventListener(
    'pointerdown',
    (event) => {
      swallowClick = false;
      // Texelscope's own UI uses Alt-click to compare pixels.
      if (!event.altKey || event.button !== 0 || document.querySelector('texelscope-app')) return;
      const canvas = event.composedPath().find((t): t is HTMLCanvasElement => t instanceof HTMLCanvasElement && contexts.has(t));
      if (!canvas) return;
      // The pick is Texelscope's, so the app doesn't see this click.
      event.preventDefault();
      event.stopImmediatePropagation();
      swallowClick = true;
      const box = canvas.getBoundingClientRect();
      const x = Math.floor(((event.clientX - box.left) * canvas.width) / box.width);
      const y = Math.floor(((event.clientY - box.top) * canvas.height) / box.height);
      void pick(canvas, contexts.get(canvas)!, x, y);
    },
    { capture: true },
  );
  for (const type of ['pointerup', 'mousedown', 'mouseup', 'click']) {
    addEventListener(
      type,
      (event) => {
        if (!swallowClick) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (type === 'click') swallowClick = false;
      },
      { capture: true },
    );
  }
}

/** Remembers which canvases hold WebGL2 or WebGPU contexts. Asking a canvas directly would make one. */
function trackContexts(): WeakMap<object, 'webgl2' | 'webgpu'> {
  const contexts = new WeakMap<object, 'webgl2' | 'webgpu'>();
  for (const proto of [globalThis.HTMLCanvasElement?.prototype, globalThis.OffscreenCanvas?.prototype]) {
    if (!proto) continue;
    const original = proto.getContext as (this: object, kind: string, ...rest: unknown[]) => unknown;
    (proto as { getContext: unknown }).getContext = function (this: object, kind: string, ...rest: unknown[]) {
      const context = original.call(this, kind, ...rest);
      if (context && (kind === 'webgl2' || kind === 'webgpu')) contexts.set(this, kind);
      return context;
    };
  }
  return contexts;
}

function uiUrl(options: AutoOptions): string {
  const given = typeof options.ui === 'function' ? options.ui() : options.ui;
  return new URL(given ?? DEFAULT_UI_URL, location.href).href;
}

function within<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    promise.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}
