import { animation, useAnimation } from '../animation.js';
import type { Capture, FragmentInputs } from '../capture.js';
import { captureDraw } from './capture.js';
import { installPatches } from './install.js';
import { drawTarget, readRect } from './read.js';
import { type DrawCall, programShaders, shaderSources } from './state.js';

/** Options for {@link installWebGL2}. */
export interface WebGL2InstallOptions {
  /** Picks the programs to watch. Every program is watched by default. */
  filter?: (program: { vertexSource: string; fragmentSource: string }) => boolean;
  /** Unused for WebGL2, which reads each draw's inputs from the GPU. */
  inputs?: FragmentInputs;
  /** How many pixels on a side the window of recorded inputs covers. Defaults to 128. */
  window?: number;
}

/** A live install of the WebGL2 hook. */
export interface WebGL2Session {
  /** Resolves with a capture of the next draw that uses a watched program. */
  captureNextDraw(): Promise<Capture>;
  /**
   * Watches the next frame drawn to `canvas` and captures every draw that changed the pixel
   * at (x, y), counted in drawing buffer pixels from the top left. The last capture is the
   * draw that left the pixel as it is. Rejects when nothing draws within `timeout` ms.
   */
  captureAt(canvas: HTMLCanvasElement | OffscreenCanvas, x: number, y: number, options?: { timeout?: number }): Promise<Capture[]>;
  /** Puts the original WebGL2 methods back. */
  uninstall(): void;
}

/** One pixel being watched through a frame. */
interface History {
  canvas: HTMLCanvasElement | OffscreenCanvas;
  x: number;
  y: number;
  /** Whether the end of the frame that shows the watched draws is on its way. */
  ending: boolean;
  draws: number;
  before?: Uint8Array | Float32Array;
  captures: Capture[];
  problems: string[];
  resolve(captures: Capture[]): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

let active: WebGL2Session | undefined;
const ids = new WeakMap<object, number>();
let nextId = 1;
const idOf = (o: object) => {
  if (!ids.has(o)) ids.set(o, nextId++);
  return ids.get(o)!;
};

/**
 * Patches WebGL2 in this global scope, a page or a worker, so draws can be captured.
 * Call it before the app compiles its shaders. A second call, say from the app after the
 * Texelscope extension or Vite plugin installed the hook, gets the session already running.
 */
export function installWebGL2(options: WebGL2InstallOptions = {}): WebGL2Session {
  if (typeof WebGL2RenderingContext === 'undefined') throw new Error('WebGL2 is not available in this scope');
  if (active) return active;

  const waiting: { resolve: (capture: Capture) => void; reject: (error: unknown) => void }[] = [];
  let history: History | undefined;
  const release = useAnimation();

  const watched = (gl: WebGL2RenderingContext) => {
    if (!options.filter) return true;
    const program = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
    const stages = program && programShaders.get(program);
    const vertexSource = stages?.vertex ? shaderSources.get(stages.vertex) : undefined;
    const fragmentSource = stages?.fragment ? shaderSources.get(stages.fragment) : undefined;
    return vertexSource !== undefined && fragmentSource !== undefined && options.filter({ vertexSource, fragmentSource });
  };

  /** The watched pixel's value in the bound target, when that target is the canvas's size. */
  const pixel = (gl: WebGL2RenderingContext, h: History) => {
    const target = drawTarget(gl);
    if (!target.readable || target.width !== gl.drawingBufferWidth || target.height !== gl.drawingBufferHeight) return undefined;
    return readRect(gl, target, h.x, target.height - 1 - h.y, 1, 1);
  };

  const label = (gl: WebGL2RenderingContext, h: History) => {
    const program = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
    const framebuffer = gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
    const into = framebuffer ? `framebuffer ${idOf(framebuffer)}` : 'the canvas';
    return `draw ${h.draws} of frame ${animation.count}: program ${program ? idOf(program) : '?'} into ${into}`;
  };

  const finish = (h: History, timeout?: number) => {
    clearTimeout(h.timer);
    if (history !== h) return;
    history = undefined;
    if (h.captures.length > 0) h.resolve(h.captures);
    else if (h.problems.length > 0) h.reject(new Error(`no draw could be captured: ${h.problems.join('; ')}`));
    else if (timeout !== undefined) h.reject(new Error(`the app drew nothing within ${timeout / 1000} s. If it draws only when something changes, change something and pick the pixel again.`));
    else h.reject(new Error(`no draw in the next frame changed pixel ${h.x}, ${h.y}, so it shows what the canvas was cleared to.`));
  };

  const restoreDraws = installPatches({
    beforeDraw(gl) {
      const h = history;
      if (h && gl.canvas === h.canvas) h.before = pixel(gl, h);
    },
    afterDraw(gl, call: DrawCall) {
      animation.drew();
      const h = history;
      if (h && gl.canvas === h.canvas) {
        h.draws++;
        if (!h.ending) {
          h.ending = true;
          animation.afterFrame(() => finish(h));
        }
        const after = pixel(gl, h);
        const before = h.before;
        h.before = undefined;
        if (before && after && before.some((v, i) => !Object.is(v, after[i]))) {
          try {
            h.captures.push(captureDraw(gl, call, { pixel: [h.x, h.y], label: label(gl, h), frame: animation.count, window: options.window }));
          } catch (error) {
            h.problems.push((error as Error).message);
          }
        }
      }
      if (waiting.length > 0 && watched(gl)) {
        const waiter = waiting.shift()!;
        try {
          waiter.resolve(captureDraw(gl, call, { frame: animation.count, window: options.window }));
        } catch (error) {
          waiter.reject(error);
        }
      }
    },
  });

  active = {
    captureNextDraw() {
      return new Promise<Capture>((resolve, reject) => waiting.push({ resolve, reject }));
    },
    captureAt(canvas, x, y, { timeout = 5000 } = {}) {
      if (history) {
        clearTimeout(history.timer);
        history.reject(new Error('a newer pixel was picked'));
      }
      return new Promise<Capture[]>((resolve, reject) => {
        const h: History = {
          canvas,
          x: Math.floor(x),
          y: Math.floor(y),
          ending: false,
          draws: 0,
          captures: [],
          problems: [],
          resolve,
          reject,
          timer: setTimeout(() => finish(h, timeout), timeout),
        };
        history = h;
      });
    },
    uninstall() {
      restoreDraws();
      release();
      active = undefined;
      for (const waiter of waiting.splice(0)) waiter.reject(new Error('the WebGL2 hook was uninstalled'));
      if (history) finish(history);
    },
  };
  return active;
}
