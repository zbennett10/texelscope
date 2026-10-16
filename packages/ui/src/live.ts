// Asks the page a WebGL2 capture came from to draw it again: with edits and what-ifs, with a
// probe, around another pixel to record its inputs, or in the app's own frames. Only the
// page holds the draw's vertices, textures and GPU, so it does the drawing.

import type { FragmentInputs } from '@texelscope/sdk';

/** What a capture message names its draw by. */
export interface LiveDraw {
  page: string;
  draw: number;
}

/** A draw as the page drew it again. */
export interface Replayed {
  width: number;
  height: number;
  /** Four floats per pixel, rows from the top, NaN where the draw didn't reach a pixel. */
  data: Float32Array;
}

/** A frame of the app's canvas, rows from the top. */
export interface CanvasFrame {
  page: string;
  draw: number;
  width: number;
  height: number;
  rgba: Uint8Array;
}

type Reply = { type: string; id: number; error?: string } & Record<string, unknown>;

interface Lane {
  running: boolean;
  waiting?: { draw: LiveDraw; source: string; resolve: (result: Replayed | string | null) => void };
}

/** How long to wait for the page before saying it didn't answer. */
const TIMEOUT = 15000;

export class LivePage {
  readonly window: Window;
  readonly origin: string;
  #pending = new Map<number, (reply: Reply | undefined) => void>();
  #next = 1;
  /** Whether a replay is in flight in each lane, and the newest one waiting behind it. */
  #lanes = new Map<string, Lane>();

  constructor(window: Window, origin: string) {
    this.window = window;
    this.origin = origin;
  }

  /** Takes a reply from the page. Returns whether the message was one. */
  receive(data: unknown): boolean {
    const reply = data as Reply | undefined;
    if (typeof reply?.id !== 'number' || typeof reply.type !== 'string' || !reply.type.endsWith('-done')) return false;
    const done = this.#pending.get(reply.id);
    this.#pending.delete(reply.id);
    done?.(reply);
    return true;
  }

  #ask(message: Record<string, unknown>): Promise<Reply | string> {
    const id = this.#next++;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        resolve("Your app's page didn't answer. If it reloaded or closed, Alt-click the pixel again.");
      }, TIMEOUT);
      this.#pending.set(id, (reply) => {
        clearTimeout(timer);
        resolve(reply?.error ?? reply ?? 'no answer');
      });
      try {
        this.window.postMessage({ ...message, id }, this.origin);
      } catch (error) {
        clearTimeout(timer);
        this.#pending.delete(id);
        resolve((error as Error).message);
      }
    });
  }

  /**
   * Has the page draw `draw` with `source` as its fragment shader. Within a lane only the newest
   * request waits behind the one in flight, and the ones it replaces resolve to null.
   */
  replay(draw: LiveDraw, source: string, lane: string): Promise<Replayed | string | null> {
    const state = this.#lanes.get(lane) ?? { running: false };
    this.#lanes.set(lane, state);
    if (state.running) {
      state.waiting?.resolve(null);
      return new Promise((resolve) => (state.waiting = { draw, source, resolve }));
    }
    return this.#replay(state, draw, source);
  }

  async #replay(state: Lane, draw: LiveDraw, source: string): Promise<Replayed | string> {
    state.running = true;
    try {
      const reply = await this.#ask({ type: 'texelscope:replay', ...draw, source });
      if (typeof reply === 'string') return reply;
      return { width: reply.width as number, height: reply.height as number, data: reply.data as Float32Array };
    } finally {
      state.running = false;
      const next = state.waiting;
      state.waiting = undefined;
      if (next) void this.#replay(state, next.draw, next.source).then(next.resolve);
    }
  }

  /** Has the page record the draw's inputs in a window around `pixel`. */
  async inputs(draw: LiveDraw, pixel: [number, number]): Promise<FragmentInputs | string> {
    const reply = await this.#ask({ type: 'texelscope:inputs', ...draw, pixel });
    return typeof reply === 'string' ? reply : (reply.inputs as FragmentInputs);
  }

  /**
   * Has the app run `source` in place of the draw's fragment shader in all its own draws, and
   * send its canvas after each frame. Null puts the app's shader back. Returns why it failed.
   */
  async scene(draw: LiveDraw, source: string | null): Promise<string | undefined> {
    const reply = await this.#ask({ type: 'texelscope:scene', ...draw, source });
    return typeof reply === 'string' ? reply : undefined;
  }

  /** Tells the page a frame it sent is on screen, so it sends the next. */
  frameShown() {
    try {
      this.window.postMessage({ type: 'texelscope:frame-shown' }, this.origin);
    } catch {
      // The page is gone.
    }
  }
}
