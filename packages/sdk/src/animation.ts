// Follows the page's animation frames, so a capture knows when the frame it watches has
// ended, and an app that draws only when something changes can be asked to draw again.

import { patcher } from './webgpu/patch.js';

/** One run of the app's frame callbacks. */
interface Frame {
  /** The timestamp the browser gave every callback of the frame. */
  time: number;
  callbacks: FrameRequestCallback[];
  /** Whether a draw happened while the callbacks ran. */
  drew: boolean;
}

/** The page's animation frames, as the app sees them. */
export interface Animation {
  /** How many frames have run since the hooks started. */
  readonly count: number;
  /** How many draws the hooks have seen. */
  readonly draws: number;
  /** Notes a draw. The WebGL2 and WebGPU hooks call it. */
  drew(): void;
  /** Calls `callback` once the frame that shows what was drawn so far has finished. */
  afterFrame(callback: () => void): void;
  /** Resolves when the browser next renders. */
  next(): Promise<void>;
  /**
   * Runs the callbacks of the last frame that drew once more, with the same timestamp, so an
   * idle app draws that frame again. Returns false when the app has a frame of its own coming,
   * which will draw anyway, or no frame drew yet.
   */
  redraw(): boolean;
}

let users = 0;
let restore: (() => void) | undefined;
let request: ((callback: FrameRequestCallback) => number) | undefined;
let count = 0;
let draws = 0;
/** The frame whose callbacks are running, until a timer after them ends it. */
let current: Frame | undefined;
let lastDrawing: Frame | undefined;
/** The app's callbacks waiting for a frame, by id. */
const waiting = new Set<number>();
const ends: (() => void)[] = [];

const end = (frame: Frame) => {
  if (current !== frame) return;
  current = undefined;
  if (frame.drew) lastDrawing = frame;
  for (const callback of ends.splice(0)) callback();
};

const run = (callback: FrameRequestCallback, time: number) => {
  // Every callback of one frame gets the same timestamp, so a new one starts a new frame.
  if (current?.time !== time) {
    if (current) end(current);
    const frame: Frame = { time, callbacks: [], drew: false };
    current = frame;
    count++;
    // A timer set now runs after the browser has run the frame's last callback.
    setTimeout(() => end(frame), 0);
  }
  current.callbacks.push(callback);
  callback(time);
};

const nextFrame = (callback: FrameRequestCallback) => (request ? request(callback) : setTimeout(() => callback(performance.now()), 16));

export const animation: Animation = {
  get count() {
    return count;
  },
  get draws() {
    return draws;
  },
  drew() {
    draws++;
    if (current) current.drew = true;
  },
  afterFrame(callback) {
    if (current) ends.push(callback);
    // A draw outside the frame callbacks shows at the next frame.
    else nextFrame(() => setTimeout(callback, 0));
  },
  next() {
    return new Promise((resolve) => nextFrame(() => resolve()));
  },
  redraw() {
    const frame = lastDrawing;
    if (!frame || current || waiting.size > 0) return false;
    nextFrame(() => {
      // The app may have asked for a frame of its own meanwhile, and that one draws.
      if (current || waiting.size > 0) return;
      for (const callback of frame.callbacks) {
        try {
          run(callback, frame.time);
        } catch (error) {
          reportError(error);
        }
      }
    });
    return true;
  },
};

/** Starts following frames, or joins the hooks already following them. Returns the release. */
export function useAnimation(): () => void {
  if (users++ === 0) restore = install();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (--users === 0) {
      restore?.();
      restore = request = undefined;
      waiting.clear();
    }
  };
}

function install(): () => void {
  const frames = patcher();
  if (typeof requestAnimationFrame === 'undefined') return frames.restore;
  const original = requestAnimationFrame;
  request = (callback) => original.call(globalThis, callback);
  frames.patch(globalThis, 'requestAnimationFrame', (raf) => (callback: FrameRequestCallback) => {
    const id = raf.call(globalThis, (time: number) => {
      waiting.delete(id);
      run(callback, time);
    });
    waiting.add(id);
    return id;
  });
  frames.patch(globalThis, 'cancelAnimationFrame', (cancel) => (id: number) => {
    waiting.delete(id);
    cancel.call(globalThis, id);
  });
  return frames.restore;
}
