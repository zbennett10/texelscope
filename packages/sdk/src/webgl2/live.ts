// Answers Texelscope about the draws this page captured. It draws one again with an edited
// shader or a probe, records the draw's inputs around another pixel, or runs an edit in the
// app's own frames and sends the canvas back after each.

import { animation } from '../animation.js';
import { answerTexelscope, isTexelscopeOpen, tellTexelscope } from '../send.js';
import { windowAround } from './capture.js';
import { windowInputs } from './inputs.js';
import { type KeptDraw, releaseDraw } from './keep.js';
import { replayDraw } from './replay.js';
import { editsRun, frameShown, restoreProgram, runEdit, streamCanvas } from './scene.js';

/** What a capture message carries for the UI to ask about its draw. */
export interface LiveDraw {
  /** Tells this page's draws from those of the page before a reload. */
  page: string;
  draw: number;
}

/** How many draws stay kept, newest first. Texelscope keeps as many captures. */
const KEPT = 24;

const page = Math.random().toString(36).slice(2);
const draws = new Map<number, KeptDraw>();
let showing: { draw: KeptDraw; stop: () => void } | undefined;
/** How many pixels on a side the hook records inputs over. */
let windowSize = 128;

/** Keeps a captured draw for Texelscope to ask about, and returns what the capture carries. */
export function keepLive(draw: KeptDraw, window: number): LiveDraw {
  windowSize = window;
  draws.set(draw.id, draw);
  for (const [id, old] of draws) {
    if (draws.size <= KEPT) break;
    draws.delete(id);
    if (showing?.draw === old) stopScene();
    releaseDraw(old);
  }
  answerTexelscope(answer);
  return { page, draw: draw.id };
}

type Request =
  | { type: 'texelscope:replay'; id: number; page: string; draw: number; source: string }
  | { type: 'texelscope:inputs'; id: number; page: string; draw: number; pixel: [number, number] }
  | { type: 'texelscope:scene'; id: number; page: string; draw: number; source: string | null }
  | { type: 'texelscope:frame-shown' };

function answer(request: { type: string }, reply: (message: unknown, transfer?: Transferable[]) => void) {
  const r = request as Request;
  if (r.type === 'texelscope:frame-shown') {
    frameShown();
    return;
  }
  if (r.type !== 'texelscope:replay' && r.type !== 'texelscope:inputs' && r.type !== 'texelscope:scene') return;
  const draw = r.page === page ? draws.get(r.draw) : undefined;
  const done = `${r.type}-done`;
  if (!draw) {
    if (r.type === 'texelscope:scene' && r.source === null) return reply({ type: done, id: r.id });
    const why = r.page === page ? 'Texelscope no longer keeps this draw' : 'the page reloaded since this capture';
    return reply({ type: done, id: r.id, error: `${why}, so Alt-click the pixel again.` });
  }
  try {
    if (r.type === 'texelscope:replay') {
      const result = replayDraw(draw, r.source);
      if (typeof result === 'string') return reply({ type: done, id: r.id, error: result });
      return reply({ type: done, id: r.id, width: result.width, height: result.height, data: result.data }, [result.data.buffer]);
    }
    if (r.type === 'texelscope:inputs') {
      const inputs = windowInputs(draw, windowAround(draw, r.pixel, windowSize));
      if (typeof inputs === 'string') return reply({ type: done, id: r.id, error: inputs });
      return reply({ type: done, id: r.id, inputs });
    }
    if (r.source === null) {
      if (showing?.draw === draw) stopScene();
      return reply({ type: done, id: r.id });
    }
    // Another draw's edit stops before this one starts.
    if (showing && showing.draw !== draw) stopScene();
    const failed = runEdit(draw.gl, draw.program, draw.vertex, r.source);
    if (failed) return reply({ type: done, id: r.id, error: failed });
    if (showing) {
      // An app that draws only when something changes draws its last frame with the new edit.
      animation.redraw();
    } else {
      const stopStream = streamCanvas(draw.gl, (frame) => {
        if (!tellTexelscope({ type: 'texelscope:frame', page, draw: draw.id, ...frame }, [frame.rgba.buffer])) stopScene();
      });
      // Once the Texelscope window closes, the app's own shader goes back, even if it is idle.
      const watch = setInterval(() => isTexelscopeOpen() || stopScene(), 1000);
      showing = {
        draw,
        stop: () => {
          clearInterval(watch);
          stopStream();
        },
      };
    }
    return reply({ type: done, id: r.id });
  } catch (error) {
    return reply({ type: done, id: r.id, error: (error as Error).message });
  }
}

/** Stops sending frames and puts back every program an edit replaced. */
function stopScene() {
  const was = showing;
  const edited = editsRun();
  showing = undefined;
  was?.stop();
  if (was) restoreProgram(was.draw.program);
  if (editsRun()) for (const d of draws.values()) restoreProgram(d.program);
  // An app that draws only when something changes draws its last frame again with its own
  // shader, once any frame running now has ended.
  if (edited) setTimeout(() => animation.redraw(), 0);
}
