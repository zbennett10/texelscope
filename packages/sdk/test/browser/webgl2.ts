// Scenes the WebGL2 browser tests draw with the hook installed, each returning plain values.

import { fromBase64 } from '../../src/encode.js';
import { connectTexelscope, markLive, openInTexelscope } from '../../src/send.js';
import { captureDraw } from '../../src/webgl2/capture.js';
import { installWebGL2 } from '../../src/webgl2/index.js';
import { windowInputs } from '../../src/webgl2/inputs.js';
import { keepLive } from '../../src/webgl2/live.js';
import type { KeptDraw } from '../../src/webgl2/keep.js';
import { replayDraw } from '../../src/webgl2/replay.js';
import { restoreProgram, runEdit } from '../../src/webgl2/scene.js';

installWebGL2();
const canvas = document.getElementById('c') as HTMLCanvasElement;
const gl = canvas.getContext('webgl2', { antialias: false, preserveDrawingBuffer: true })!;
const SIZE = 32;

const VERTEX = `#version 300 es
in vec3 a_pos;
in float a_id;
flat out float v_id;
out vec2 v_uv;
void main() {
  v_uv = a_pos.xy * 0.5 + 0.5;
  v_id = a_id;
  gl_Position = vec4(a_pos, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform vec3 u_tint;
flat in float v_id;
in vec2 v_uv;
out vec4 color;
void main() {
  color = vec4(u_tint.x * v_uv.x, v_id / 4.0, u_tint.z, 1.0);
}`;

function program(vertex: string, fragment: string): WebGLProgram {
  const p = gl.createProgram()!;
  for (const [type, source] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]] as const) {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, source);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? '');
    gl.attachShader(p, s);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? '');
  return p;
}

/** Two triangles over the middle in one draw, the near one first, each with its id. */
function overlapping(p: WebGLProgram) {
  const data = new Float32Array([
    // x, y, z, id: near (z -0.5) then far (z 0.5).
    -1, -1, -0.5, 1, 3, -1, -0.5, 1, -1, 3, -0.5, 1,
    -1, -1, 0.5, 2, 3, -1, 0.5, 2, -1, 3, 0.5, 2,
  ]);
  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);
  const buffer = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
  const pos = gl.getAttribLocation(p, 'a_pos');
  const id = gl.getAttribLocation(p, 'a_id');
  gl.enableVertexAttribArray(pos);
  gl.vertexAttribPointer(pos, 3, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(id);
  gl.vertexAttribPointer(id, 1, gl.FLOAT, false, 16, 12);
  return { vao, buffer, count: 6 };
}

function pixels(): Uint8Array {
  const out = new Uint8Array(SIZE * SIZE * 4);
  gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
  gl.readPixels(0, 0, SIZE, SIZE, gl.RGBA, gl.UNSIGNED_BYTE, out);
  return out;
}

/** A pixel from the top left of RGBA bytes read bottom row first. */
const at = (rgba: Uint8Array, x: number, y: number) => Array.from(rgba.subarray(((SIZE - 1 - y) * SIZE + x) * 4, ((SIZE - 1 - y) * SIZE + x) * 4 + 4));
/** A pixel from the top left of floats with rows from the top. */
const floatAt = (data: Float32Array, x: number, y: number) => Array.from(data.subarray((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4));

/** Draws the overlapping triangles with a depth test and keeps the draw. */
function depthScene(tint: [number, number, number] = [0.5, 1, 0.25]) {
  const p = program(VERTEX, FRAGMENT);
  const mesh = overlapping(p);
  gl.viewport(0, 0, SIZE, SIZE);
  gl.clearColor(0, 0, 0, 0);
  gl.clearDepth(1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  gl.enable(gl.DEPTH_TEST);
  gl.depthFunc(gl.LESS);
  gl.useProgram(p);
  gl.uniform3fv(gl.getUniformLocation(p, 'u_tint'), tint);
  gl.drawArrays(gl.TRIANGLES, 0, mesh.count);
  const { capture, kept } = captureDraw(gl, { method: 'drawArrays', args: [gl.TRIANGLES, 0, mesh.count] }, 1, { pixel: [16, 16], window: 8 });
  return { p, mesh, capture, kept };
}

function windowValue(capture: { inputs?: unknown }, name: string, x: number, y: number): number[] {
  const inputs = capture.inputs as { rect: { x: number; y: number; width: number }; varyings: { name: string; components: number; data: string }[] };
  const v = inputs.varyings.find((i) => i.name === name)!;
  const all = new Float32Array(fromBase64(v.data).buffer);
  const p = (y - inputs.rect.y) * inputs.rect.width + (x - inputs.rect.x);
  return Array.from(all.subarray(p * v.components, (p + 1) * v.components));
}

const STATE = () => ({
  program: gl.getParameter(gl.CURRENT_PROGRAM),
  vao: gl.getParameter(gl.VERTEX_ARRAY_BINDING),
  array: gl.getParameter(gl.ARRAY_BUFFER_BINDING),
  draw: gl.getParameter(gl.DRAW_FRAMEBUFFER_BINDING),
  read: gl.getParameter(gl.READ_FRAMEBUFFER_BINDING),
  viewport: Array.from(gl.getParameter(gl.VIEWPORT)),
  depth: gl.isEnabled(gl.DEPTH_TEST),
  depthFunc: gl.getParameter(gl.DEPTH_FUNC),
  blend: gl.isEnabled(gl.BLEND),
  scissor: gl.isEnabled(gl.SCISSOR_TEST),
  unit: gl.getParameter(gl.ACTIVE_TEXTURE),
  texture: gl.getParameter(gl.TEXTURE_BINDING_2D),
  renderbuffer: gl.getParameter(gl.RENDERBUFFER_BINDING),
  colorMask: Array.from(gl.getParameter(gl.COLOR_WRITEMASK)),
  error: gl.getError(),
});

/** Polls for up to two seconds until `find` returns something. */
async function waitFor<T>(find: () => T | undefined): Promise<T | undefined> {
  for (let i = 0; i < 100; i++) {
    const found = find();
    if (found !== undefined) return found;
    await new Promise((r) => setTimeout(r, 20));
  }
  return undefined;
}

type FakeUi = Window & { received: { type: string; id?: number }[]; ask(m: unknown): void };

/** Sends the scene's capture to a stand-in Texelscope window, which the page then answers. */
async function openFakeUi(s: ReturnType<typeof depthScene>) {
  const live = keepLive(s.kept, 8);
  markLive(s.capture, live);
  connectTexelscope(new URL('fake-ui.html', location.href).href);
  await openInTexelscope(s.capture, { focus: false });
  const ui = window.open('', 'texelscope') as FakeUi;
  const reply = (id: number) => waitFor(() => ui.received.find((m) => m.id === id) as Record<string, unknown> | undefined);
  const sent = await waitFor(() => ui.received.find((m) => m.type === 'texelscope:capture') as { live?: unknown } | undefined);
  return { live, ui, reply, sent };
}

/** The middle pixel of a canvas frame the page sent, whose rows run from the top. */
const frameMiddle = (frame: { width: number; rgba: Uint8Array }) => Array.from(frame.rgba.subarray((16 * frame.width + 16) * 4, (16 * frame.width + 16) * 4 + 4));

let kept: KeptDraw | undefined;

const scenes = {
  /** The inputs at a pixel two triangles of one draw cover come from the near one. */
  nearestInputs() {
    const s = depthScene();
    kept = s.kept;
    return { app: at(pixels(), 16, 16), id: windowValue(s.capture, 'v_id', 16, 16) };
  },

  /** The draw again with its own shader, next to what the app drew. */
  replayed() {
    const s = depthScene();
    const app = pixels();
    const result = replayDraw(s.kept, s.kept.source);
    if (typeof result === 'string') throw new Error(result);
    const worst = { diff: 0 };
    for (let y = 0; y < SIZE; y++) {
      for (let x = 0; x < SIZE; x++) {
        const a = at(app, x, y);
        const r = floatAt(result.data, x, y).map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255));
        worst.diff = Math.max(worst.diff, ...a.map((v, i) => Math.abs(v - r[i]!)));
      }
    }
    return { worst: worst.diff, middle: floatAt(result.data, 16, 16) };
  },

  /** An edit, and the uniform values the draw had even after the app changes them. */
  edited() {
    const s = depthScene([0.5, 1, 0.25]);
    gl.useProgram(s.p);
    gl.uniform3f(gl.getUniformLocation(s.p, 'u_tint'), 0, 0, 0);
    const edit = s.kept.source.replace('color = vec4(u_tint.x * v_uv.x, v_id / 4.0, u_tint.z, 1.0);', 'color = vec4(u_tint.yxz, 0.75);');
    const result = replayDraw(s.kept, edit);
    if (typeof result === 'string') throw new Error(result);
    const broken = replayDraw(s.kept, edit.replace('0.75', 'nope'));
    return { middle: floatAt(result.data, 16, 16), broken: typeof broken === 'string' ? broken : 'compiled' };
  },

  /** The app's state is the same after a replay and a recording of inputs elsewhere. */
  stateKept() {
    const s = depthScene();
    const other = gl.createVertexArray();
    gl.bindVertexArray(other);
    gl.activeTexture(gl.TEXTURE3);
    gl.enable(gl.BLEND);
    gl.viewport(1, 2, 3, 4);
    const before = STATE();
    const replay = replayDraw(s.kept, s.kept.source);
    const inputs = windowInputs(s.kept, { x: 0, y: 0, width: 4, height: 4 });
    const after = STATE();
    const changed = (Object.keys(before) as (keyof typeof before)[]).filter((k) => {
      const [a, b] = [before[k], after[k]];
      return Array.isArray(a) ? JSON.stringify(a) !== JSON.stringify(b) : a !== b;
    });
    return { replayed: typeof replay !== 'string', recorded: typeof inputs !== 'string', changed, error: after.error };
  },

  /** A draw that samples a render target sees what it held at the capture. */
  renderTarget() {
    const target = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, target);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 4, 4, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    const fbo = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
    gl.clearColor(1, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    const p = program(VERTEX, `#version 300 es
precision highp float;
uniform sampler2D u_image;
in vec2 v_uv;
out vec4 color;
void main() { color = texture(u_image, v_uv); }`);
    const mesh = overlapping(p);
    gl.disable(gl.DEPTH_TEST);
    gl.viewport(0, 0, SIZE, SIZE);
    gl.useProgram(p);
    gl.uniform1i(gl.getUniformLocation(p, 'u_image'), 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, target);
    gl.drawArrays(gl.TRIANGLES, 0, mesh.count);
    const { kept } = captureDraw(gl, { method: 'drawArrays', args: [gl.TRIANGLES, 0, mesh.count] }, 2, { pixel: [16, 16], window: 8 });
    // The next frame fills the target with green.
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.clearColor(0, 1, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    const result = replayDraw(kept, kept.source);
    if (typeof result === 'string') throw new Error(result);
    return floatAt(result.data, 16, 16);
  },

  /** The app runs an edit in its own draws, with the uniforms it sets through its locations. */
  swapped() {
    const s = depthScene();
    const tint = gl.getUniformLocation(s.p, 'u_tint');
    const drawAgain = (value: [number, number, number]) => {
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.useProgram(s.p);
      gl.uniform3fv(tint, value);
      gl.bindVertexArray(s.mesh.vao);
      gl.drawArrays(gl.TRIANGLES, 0, s.mesh.count);
      return at(pixels(), 16, 16);
    };
    const edit = s.kept.source.replace('color = vec4(u_tint.x * v_uv.x, v_id / 4.0, u_tint.z, 1.0);', 'color = vec4(u_tint, 1.0);');
    const failed = runEdit(gl, s.p, s.kept.vertex, edit);
    const edited = drawAgain([0, 1, 0]);
    restoreProgram(s.p);
    const back = drawAgain([0, 1, 0]);
    return { failed: failed ?? null, edited, back, error: gl.getError() };
  },

  /** Inputs recorded later around another pixel, after the app bound other vertex arrays. */
  movedWindow() {
    const s = depthScene();
    gl.bindVertexArray(gl.createVertexArray());
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    const inputs = windowInputs(s.kept, { x: 24, y: 24, width: 8, height: 8 });
    if (typeof inputs === 'string') throw new Error(inputs);
    return { uv: windowValue({ inputs }, 'v_uv', 28, 28), id: windowValue({ inputs }, 'v_id', 28, 28) };
  },

  /** The page answers the window it opened, and only that window, about a capture's draw. */
  async liveChannel() {
    const s = depthScene();
    const { live, ui, reply, sent } = await openFakeUi(s);
    ui.ask({ type: 'texelscope:replay', id: 1, ...live, source: s.kept.source });
    const replayed = await reply(1);
    ui.ask({ type: 'texelscope:inputs', id: 2, ...live, pixel: [3, 3] });
    const inputs = (await reply(2))?.inputs as { rect: unknown } | undefined;
    ui.ask({ type: 'texelscope:replay', id: 3, page: 'another', draw: live.draw, source: s.kept.source });
    const reloaded = await reply(3);
    // A message from this page itself, rather than the window it opened, gets no answer.
    window.postMessage({ type: 'texelscope:replay', id: 4, ...live, source: s.kept.source }, location.origin);
    const stranger = await reply(4);
    const edit = s.kept.source.replace('color = vec4(u_tint.x * v_uv.x, v_id / 4.0, u_tint.z, 1.0);', 'color = vec4(0.0, 0.0, 1.0, 1.0);');
    ui.ask({ type: 'texelscope:scene', id: 5, ...live, source: edit });
    const scene = await reply(5);
    // The app draws its next frame, and the canvas comes back with the edit in it.
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.useProgram(s.p);
    gl.bindVertexArray(s.mesh.vao);
    gl.drawArrays(gl.TRIANGLES, 0, s.mesh.count);
    await new Promise((r) => setTimeout(r, 50));
    const frame = ui.received.find((m) => m.type === 'texelscope:frame') as { width: number; rgba: Uint8Array } | undefined;
    ui.ask({ type: 'texelscope:scene', id: 6, ...live, source: null });
    await reply(6);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, s.mesh.count);
    const back = at(pixels(), 16, 16);
    ui.close();
    return {
      live: sent?.live ?? null,
      replayed: replayed && { width: replayed.width, middle: floatAt(replayed.data as Float32Array, 16, 16) },
      window: inputs?.rect ?? null,
      reloaded: reloaded?.error ?? null,
      stranger: stranger ?? null,
      scene: scene && !scene.error,
      frame: frame && frameMiddle(frame),
      back,
    };
  },

  /** An app that draws only when something changes, shown with one edit and then another. */
  async idleEdits() {
    const s = depthScene();
    // The app draws in one frame callback and then waits for something to change.
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => {
        gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
        gl.useProgram(s.p);
        gl.bindVertexArray(s.mesh.vao);
        gl.drawArrays(gl.TRIANGLES, 0, s.mesh.count);
        resolve();
      }),
    );
    const { live, ui, reply } = await openFakeUi(s);
    const solid = (rgb: string) => s.kept.source.replace('color = vec4(u_tint.x * v_uv.x, v_id / 4.0, u_tint.z, 1.0);', `color = vec4(${rgb}, 1.0);`);
    const shows = async (rgba: number[]) =>
      (await waitFor(() => ui.received.some((m) => m.type === 'texelscope:frame' && frameMiddle(m as never).join() === rgba.join()) || undefined)) ?? false;
    ui.ask({ type: 'texelscope:scene', id: 1, ...live, source: solid('0.0, 0.0, 1.0') });
    await reply(1);
    const blue = await shows([0, 0, 255, 255]);
    ui.ask({ type: 'texelscope:frame-shown' });
    ui.ask({ type: 'texelscope:scene', id: 2, ...live, source: solid('1.0, 0.0, 0.0') });
    await reply(2);
    const red = await shows([255, 0, 0, 255]);
    ui.ask({ type: 'texelscope:scene', id: 3, ...live, source: null });
    await reply(3);
    // The app's own shader draws green as v_id / 4 for the near triangle.
    const back = (await waitFor(() => (at(pixels(), 16, 16)[1] === 64 ? true : undefined))) ?? false;
    ui.close();
    return { blue, red, back };
  },
};

Object.assign(window, { scenes, ready: true, keptDraw: () => kept });
