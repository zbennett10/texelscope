# Texelscope

Texelscope is a debugger for WGSL and GLSL shaders that explains why a pixel has its color. It replays a captured draw on the GPU and on a reference interpreter, then traces any pixel from the data it reads to the color it writes. Edit the shader, pin a value or flip a decision, and the image and the explanation update together.

## What it shows

- Why a pixel has its color, in a few plain sentences that say where the color was read, which `clamp` held a value at its bound, which side of a `mix` was kept and which branches set the path.
- The color the GPU wrote next to the one the interpreter computed.
- Every input, uniform, texel, `let` and `var` the color depends on, with its value and the code that produced it, in the order the shader computed them. Map shows any of them across the whole image.
- The decisions along the way, each with how close it came to flipping.
- The inputs that move the color most.
- A trace to step through, with values inline in the source and the lines the pixel never ran dimmed.
- Where two pixels part ways, and the decisions and values that differ.
- The values and decisions that vary across a region, each drawn as a small image in the order a pixel computes them, so you can watch the picture build up from data to color.
- The stage where an edge in a region first appears and the stage where it is complete.

## Getting started

You need Rust 1.88 or newer, wasm-pack and Node 20.19 or newer.

```sh
npm install
npm run dev
```

The UI runs on port 5173. Press Example to load a sample capture, or open your own with Open capture or by dropping the file on the page.

## Using the UI

- Click a pixel to explain it.
- Alt-click another pixel to compare the two.
- Shift-drag to inspect a region.
- Scroll to zoom, drag to pan and double-click to fit.
- Press the left and right arrows to step through frames.
- Drag the edge of a side pane to resize it, or double-click the edge to reset it.
- For a WebGL2 capture from your app, the buttons over the image switch between Frame, Draw and Live.
- The button at the right end of the header switches between the dark and light themes.

## Debugging your app

With the Vite plugin or the Chrome extension, Alt-click any pixel of a WebGL2 or WebGPU canvas and Texelscope opens with the draw behind it. The app needs no Texelscope code.

### Vite plugin

The plugin starts the hooks before the app's own scripts and serves the UI from the dev server at `/__texelscope/`. Until the packages are published, import it from a Texelscope checkout and run `npm run build` there once.

```ts
// vite.config.ts in the app
import { defineConfig } from 'vite';
import texelscope from '../texelscope/packages/vite-plugin/src/index.ts';

export default defineConfig({ plugins: [texelscope()] });
```

### Chrome extension

Run `npm run build`, open `chrome://extensions`, turn on developer mode and load `packages/extension/dist` with Load unpacked. The extension carries its own copy of the UI and runs on localhost and 127.0.0.1. For another site, turn it on from the extension's menu on that site. Turning it on reloads the page, since the hooks have to start before the app's scripts.

### What Alt-click captures

In WebGL2, Alt-click captures every draw in the next frame that changed the pixel, and each one arrives as a frame. In WebGPU it captures the next draw, which may belong to a different pass than the one behind the pixel. The SDK's `filter` option picks the pass. If the app only draws when something changes, Texelscope asks it to draw its last frame again, and if that fails, a note on the page asks you to move the view a little.

### Editing a WebGL2 draw

Texelscope opens in a window beside your app, and the page that sent a WebGL2 capture keeps what the draw used. When you edit the shader, pin a value or flip a decision, your app draws it again on its own GPU and Texelscope shows the result.

- Frame shows the frame your app drew when you picked the pixel.
- Draw shows this draw by itself, drawn again with your edits and what-ifs.
- Live runs your edits in every draw that uses the shader, so the whole app shows them. Your app's own shader comes back when you leave Live or close Texelscope.

Map works the same way, and clicking outside the window of recorded inputs records them around the new pixel. The draw keeps the uniforms and render targets it had when you picked the pixel, so it matches the explanation while your app animates. In the explanation, and whenever your app draws an edit, loops stop after 16384 turns in all, so a flip that keeps a loop going can't hang the GPU.

To try it on a small scene, run `npm run build`, then `npx vite examples/webgl2-scene`, open the page and Alt-click a box.

### From code

Install the hook before the app requests its GPU device, then capture a draw and send it to the UI.

```ts
import { installWebGPU, openInTexelscope } from '@texelscope/sdk';

const texelscope = installWebGPU({ filter: (pipeline) => pipeline.label === 'my-pass' });

// Later, for example from a key handler.
const capture = await texelscope.captureNextDraw();
await openInTexelscope(capture, { pixel: [x, y] });
```

The hook also works in a worker. Post the capture to the page and send it from there. `downloadCapture` saves a capture as a file that the UI and the command line can open. Fragment inputs other than builtins go in the `inputs` option, since the hook can't see the vertex stage.

WebGL2 works the same way and can also follow one pixel through a frame.

```ts
import { installWebGL2, openInTexelscope } from '@texelscope/sdk';

const gl2 = installWebGL2();

// Every draw in the next frame that changed the pixel, counted from the top left.
const captures = await gl2.captureAt(canvas, x, y);
await openInTexelscope(captures[captures.length - 1], { pixel: [x, y] });
```

The WebGL2 hook reads each draw's uniforms, textures and samplers back from the GPU, then runs the draw again to record its fragment inputs in a window around the pixel, 128 pixels on a side by default.

## Command line

```sh
cargo run -p texelscope-cli -- check examples/scalar-field/field.wgsl
cargo run -p texelscope-cli -- emit examples/scalar-field/field.wgsl --to glsl-es
cargo run -p texelscope-cli -- capture examples/scalar-field/capture.json
cargo run -p texelscope-cli -- trace examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- explain examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- compare examples/scalar-field/capture.json --pixel 100,128 --with 140,128
cargo run -p texelscope-cli -- render examples/scalar-field/capture.json -o field.png
```

`trace`, `explain` and `compare` take `--json` for the full data and `--overrides` with a JSON file of pins and forced branches. `render` shades every pixel on the CPU, which gives a reference image to compare with the GPU's.

## Shader support

The interpreter runs WGSL fragment shaders with scalars, vectors, matrices, structs, arrays, pointers, loops, switches, function calls, every math builtin, `textureLoad`, filtered and mipmapped `textureSample` variants and derivatives. It shades the four pixels of a quad together, so `dpdx`, `dpdy` and `fwidth` match the GPU. GLSL ES 3.00 shaders from WebGL2 go through naga's GLSL front end.

## Development

```sh
cargo test --workspace
npm test
npm run build
```

`npm run build` builds every package and puts the extension in `packages/extension/dist`.

| Path | Contents |
|---|---|
| `crates/texelscope-capture` | The capture format, loading, validation and texel decoding |
| `crates/texelscope-ir` | WGSL and GLSL parsing and validation through naga, expression ids and values |
| `crates/texelscope-interp` | The reference interpreter, its trace format and region inspection |
| `crates/texelscope-explain` | Explanations built from traces |
| `crates/texelscope-rewrite` | Code emission, what-if overrides and GPU probes |
| `crates/texelscope-cli` | The `texelscope` command |
| `crates/texelscope-wasm` | Bindings for the browser |
| `packages/sdk` | In-page hooks for WebGPU and WebGL2 |
| `packages/ui` | The web UI, built from Lit web components |
| `packages/vite-plugin` | A Vite plugin that adds the hooks to an app while it is in development |
| `packages/extension` | A Chrome extension that adds the hooks to any page |
| `examples` | Synthetic shaders, captures and a WebGL2 scene |
| `docs` | The capture format |

## Not yet

Following a pixel through a frame in WebGPU, a Firefox build of the extension and a whole-frame comparison of the GPU and the interpreter are next.

## License

[MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.
