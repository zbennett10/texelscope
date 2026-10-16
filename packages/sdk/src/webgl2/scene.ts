// Runs an edited fragment shader in place of the app's own, in every draw the app makes with
// that program, so the whole frame shows the change, later passes included. The uniform
// locations the app holds for its program reach the edited one by name. The canvas goes
// back to Texelscope after each frame that drew to it.

import { animation } from '../animation.js';
import { patcher } from '../webgpu/patch.js';
import { linkWith, readUniforms, writeUniforms } from './program.js';
import { flipRows } from './read.js';
import { owned, uniformNames } from './state.js';

interface Swap {
  gl: WebGL2RenderingContext;
  edited: WebGLProgram;
  /** The edited program's location for each uniform name the app sets. */
  locations: Map<string, WebGLUniformLocation | null>;
}

/** The app's programs that run an edit instead, while Texelscope shows the scene. */
const swaps = new Map<WebGLProgram, Swap>();
let unpatch: (() => void) | undefined;

const SETTERS = [
  'uniform1f', 'uniform1fv', 'uniform1i', 'uniform1iv', 'uniform1ui', 'uniform1uiv',
  'uniform2f', 'uniform2fv', 'uniform2i', 'uniform2iv', 'uniform2ui', 'uniform2uiv',
  'uniform3f', 'uniform3fv', 'uniform3i', 'uniform3iv', 'uniform3ui', 'uniform3uiv',
  'uniform4f', 'uniform4fv', 'uniform4i', 'uniform4iv', 'uniform4ui', 'uniform4uiv',
  'uniformMatrix2fv', 'uniformMatrix3fv', 'uniformMatrix4fv', 'uniformMatrix2x3fv', 'uniformMatrix2x4fv',
  'uniformMatrix3x2fv', 'uniformMatrix3x4fv', 'uniformMatrix4x2fv', 'uniformMatrix4x3fv',
] as const;

/** The program a WebGL call made for `program` reaches, the edit while one runs. */
const runs = (program: WebGLProgram | null) => (program && swaps.get(program)?.edited) ?? program;

/** Patches the calls that bind programs and set uniforms, while any edit runs. */
function patchCalls() {
  if (unpatch) return;
  const { patch, restore } = patcher();
  const proto = WebGL2RenderingContext.prototype;
  patch(proto, 'useProgram', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram | null) {
    return original.call(this, runs(program));
  });
  for (const name of SETTERS) {
    patch(proto, name, (original) => function (this: WebGL2RenderingContext, location: WebGLUniformLocation | null, ...rest: unknown[]) {
      const named = location && uniformNames.get(location);
      const swap = named && swaps.get(named.program);
      if (!swap || this.getParameter(this.CURRENT_PROGRAM) !== swap.edited) {
        return (original as (...a: unknown[]) => void).call(this, location, ...rest);
      }
      if (!swap.locations.has(named.name)) swap.locations.set(named.name, this.getUniformLocation(swap.edited, named.name));
      // A uniform the edit no longer uses has no location, and setting it does nothing.
      const mapped = swap.locations.get(named.name);
      if (mapped) (original as (...a: unknown[]) => void).call(this, mapped, ...rest);
    } as never);
  }
  patch(proto, 'uniformBlockBinding', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram, index: number, point: number) {
    original.call(this, program, index, point);
    const swap = swaps.get(program);
    const name = swap && this.getActiveUniformBlockName(program, index);
    const mapped = name ? this.getUniformBlockIndex(swap.edited, name) : this.INVALID_INDEX;
    if (swap && mapped !== this.INVALID_INDEX) original.call(this, swap.edited, mapped, point);
  });
  // A program the app links again or deletes stops running the edit first.
  patch(proto, 'linkProgram', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram) {
    restoreProgram(program);
    return original.call(this, program);
  });
  patch(proto, 'deleteProgram', (original) => function (this: WebGL2RenderingContext, program: WebGLProgram | null) {
    if (program) restoreProgram(program);
    return original.call(this, program);
  });
  unpatch = restore;
}

/**
 * Makes the app's `program` run `source` as its fragment shader in every draw, starting with
 * the uniform values it holds now. Returns the compile or link log when the edit fails, and
 * the app's program then keeps running as it was.
 */
export function runEdit(gl: WebGL2RenderingContext, program: WebGLProgram, vertex: WebGLShader, source: string): string | undefined {
  return owned(() => {
    const edited = linkWith(gl, program, vertex, source);
    if (typeof edited === 'string') return edited;
    restoreProgram(program);
    patchCalls();
    const current = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
    gl.useProgram(edited);
    writeUniforms(gl, edited, readUniforms(gl, program));
    copyBlockBindings(gl, program, edited);
    swaps.set(program, { gl, edited, locations: new Map() });
    // The patched call puts the edit in place when the app's program was bound.
    gl.useProgram(current);
    return undefined;
  });
}

/** Puts the app's program back in its draws, with the uniform values the app gave the edit. */
export function restoreProgram(program: WebGLProgram) {
  const swap = swaps.get(program);
  if (!swap) return;
  swaps.delete(program);
  const gl = swap.gl;
  owned(() => {
    if (gl.isContextLost()) return;
    const current = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram | null;
    gl.useProgram(program);
    writeUniforms(gl, program, readUniforms(gl, swap.edited));
    gl.useProgram(current === swap.edited ? program : current);
    gl.deleteProgram(swap.edited);
  });
  if (swaps.size === 0) {
    unpatch?.();
    unpatch = undefined;
  }
}

/** Whether any of the app's programs runs an edit. */
export function editsRun(): boolean {
  return swaps.size > 0;
}

function copyBlockBindings(gl: WebGL2RenderingContext, from: WebGLProgram, to: WebGLProgram) {
  const blocks = gl.getProgramParameter(to, gl.ACTIVE_UNIFORM_BLOCKS) as number;
  for (let i = 0; i < blocks; i++) {
    const name = gl.getActiveUniformBlockName(to, i);
    const original = name ? gl.getUniformBlockIndex(from, name) : gl.INVALID_INDEX;
    if (original === gl.INVALID_INDEX) continue;
    gl.uniformBlockBinding(to, i, gl.getActiveUniformBlockParameter(from, original, gl.UNIFORM_BLOCK_BINDING) as number);
  }
}

/** A frame of the canvas, rows from the top. */
export interface CanvasFrame {
  width: number;
  height: number;
  rgba: Uint8Array;
}

interface Stream {
  gl: WebGL2RenderingContext;
  send(frame: CanvasFrame): void;
  /** Whether the canvas has a frame that wasn't sent yet. */
  dirty: boolean;
  /** Whether a sent frame waits for Texelscope to show it. */
  waiting: boolean;
  /** Whether a frame was left out while one waited. */
  skipped: boolean;
  stop: () => void;
}

let stream: Stream | undefined;

/**
 * Sends the canvas of `gl` after each frame that draws to it, one at a time: the next waits
 * until Texelscope says it showed the last. Returns the stop.
 */
export function streamCanvas(gl: WebGL2RenderingContext, send: (frame: CanvasFrame) => void): () => void {
  stream?.stop();
  const stopListening = animation.afterCallback(flush);
  const s: Stream = {
    gl,
    send,
    dirty: false,
    waiting: false,
    skipped: false,
    stop() {
      stopListening();
      if (stream === s) stream = undefined;
    },
  };
  stream = s;
  // An app that draws only when something changes draws its last frame again.
  animation.redraw();
  return s.stop;
}

/** Notes a draw, which the WebGL2 hook reports after each one. */
export function noteDraw(gl: WebGL2RenderingContext) {
  const s = stream;
  if (!s || s.gl !== gl || gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING)) return;
  if (s.dirty) return;
  s.dirty = true;
  // A draw outside a frame callback is read once the code that made it has finished.
  if (!animation.inCallback) queueMicrotask(flush);
}

/** Texelscope showed the last frame, so the next can go. */
export function frameShown() {
  const s = stream;
  if (!s) return;
  s.waiting = false;
  if (s.skipped) {
    s.skipped = false;
    animation.redraw();
  }
}

function flush() {
  const s = stream;
  if (!s || !s.dirty) return;
  s.dirty = false;
  if (s.waiting) {
    s.skipped = true;
    return;
  }
  const gl = s.gl;
  if (gl.isContextLost()) return;
  const width = gl.drawingBufferWidth;
  const height = gl.drawingBufferHeight;
  const rgba = new Uint8Array(width * height * 4);
  owned(() => {
    const read = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING) as WebGLFramebuffer | null;
    const pack = gl.getParameter(gl.PIXEL_PACK_BUFFER_BINDING) as WebGLBuffer | null;
    const params = [gl.PACK_ALIGNMENT, gl.PACK_ROW_LENGTH, gl.PACK_SKIP_PIXELS, gl.PACK_SKIP_ROWS].map((p) => [p, gl.getParameter(p) as number] as const);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, null);
    gl.pixelStorei(gl.PACK_ALIGNMENT, 4);
    gl.pixelStorei(gl.PACK_ROW_LENGTH, 0);
    gl.pixelStorei(gl.PACK_SKIP_PIXELS, 0);
    gl.pixelStorei(gl.PACK_SKIP_ROWS, 0);
    gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, read);
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, pack);
    for (const [p, v] of params) gl.pixelStorei(p, v);
  });
  s.waiting = true;
  s.send({ width, height, rgba: flipRows(rgba, width, height, 4) });
}
