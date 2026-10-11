# Texelscope

Texelscope is a debugger for WGSL and GLSL shaders that explains why a pixel has its color. It replays a captured draw on a reference interpreter and on the GPU. You can follow any pixel or region from the data it reads to the color it writes, read an account of what decided the result, and try changes to the shader while it runs.

## What works today

A small SDK captures a draw from a running WebGPU or WebGL2 page, with its shader, its uniforms and every texel of its textures. The web UI replays the capture on the GPU and runs the same fragment shader on a CPU interpreter, so for any pixel you get the following.

- A few sentences on why the pixel has its color. They say where the color was read, which value a `clamp` or `step` held at a bound, which side of a `mix` or `select` was kept and which branches chose the path.
- The color the GPU wrote next to the color the interpreter computed, and whether they agree.
- The chain from data to color. Each fragment input, uniform, texel, `let` and `var` the color depends on appears with its value and the code that produced it, in the order the shader computed them. Values computed inside a function call fold under that call, and values that come only from settings stay hidden until you ask for them. Map on any value shows it at every pixel of the image, computed on the GPU.
- Every decision on the way, with how far its operands were from flipping it, such as "false, 13.2 from flipping".
- The inputs that move the color most, measured by nudging each one and running the pixel again.
- A full trace you can step through. Each step's value is shown beside the source that produced it, the values of named lets appear inline in the editor and the lines the pixel didn't run are dimmed.

Alt-click a second pixel to compare the two. Texelscope says where they part ways, such as a decision that went the other way or a value held at a bound in one and not the other, and lists the decisions and values that differ.

Shift-drag over the image and every named value and decision across that region appears in the order a pixel computes them. Those that differ from pixel to pixel show as images. A value runs from dark for the lowest in the region to yellow for the highest, and a decision is green where it was true and gray where it was false. Those that are the same at every pixel are listed beside the images, with the ones that come only from settings last. That shows the mapping from data to pixel one stage at a time, without debug modes in the shader. When the region holds an edge, Texelscope also says in which stage the edge starts to show and by which it is complete.

To try a change, edit the source, pin any value or flip any decision. The GPU replay and the interpreter both pick it up, so you see the new image and the new explanation together. Captures arrive as frames, and the arrow keys step through them, so a pixel can be followed over time.

The interpreter handles WGSL fragment shaders that use scalars, vectors, matrices, structs, arrays, pointers, loops, switches, function calls, every math builtin, `textureLoad`, filtered and mipmapped `textureSample` variants, and derivatives. It runs the four pixels of a quad together so `dpdx`, `dpdy` and `fwidth` match the GPU. GLSL ES 3.00 shaders from WebGL2 are read through naga's GLSL front end, with their uniforms, uniform blocks and samplers bound by name.

## Layout

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
| `examples` | Synthetic shaders and captures |
| `docs` | The capture format |

## Building

Texelscope needs Rust 1.88 or newer, wasm-pack and Node 20.19 or newer.

```sh
cargo test --workspace
npm install
npm run dev
```

`npm run dev` builds the browser bindings and serves the UI at http://localhost:5173. Press Example to load the scalar-field capture. `npm run build` builds every package, with the extension in `packages/extension/dist`.

## Debugging an app without changing it

With the Vite plugin or the Chrome extension, Alt-click any pixel of a WebGL2 or WebGPU canvas and Texelscope opens with the draw behind it. The app needs no Texelscope code.

The Vite plugin starts the hooks before the app's own scripts while the dev server runs, and serves the Texelscope UI from that server at `/__texelscope/`. Until the packages are published, import it from a Texelscope checkout, and run `npm run build` there once so the UI exists.

```ts
// vite.config.ts in the app
import { defineConfig } from 'vite';
import texelscope from '../texelscope/packages/vite-plugin/src/index.ts';

export default defineConfig({ plugins: [texelscope()] });
```

The extension carries its own copy of the UI. After `npm run build`, open chrome://extensions, turn on developer mode and load `packages/extension/dist` as an unpacked extension. It runs on localhost and 127.0.0.1, which covers most dev servers. For any other site, open the extension's menu on a page of that site and turn it on there. The page reloads, since the hooks have to start before the app's scripts.

For WebGL2, Alt-click watches the next frame and captures every draw that changed the pixel, so a pixel that several passes drew arrives as several frames, with the last one open. For WebGPU it captures the next draw, which in an app with several render passes may be a different pass from the one behind the pixel. The SDK's `filter` option picks the pass when that matters. An app that draws only when something changes is asked to draw its last frame again, and if that doesn't work, a note in the corner of the page asks you to change something, such as moving the view a little. On Texelscope's own page, Alt-click still compares pixels.

## Debugging from code

Install the hook before the page asks for its GPU device, so it sees every resource the app creates. Then capture a draw and send it to the UI.

```ts
import { installWebGPU, openInTexelscope } from '@texelscope/sdk';

const texelscope = installWebGPU({ filter: (pipeline) => pipeline.label === 'my-pass' });

// Later, for example from a key handler.
const capture = await texelscope.captureNextDraw();
await openInTexelscope(capture, { pixel: [x, y] });
```

The hook patches WebGPU's prototypes, so it works in a page or a worker. Captures from a worker can be posted to the page and sent on from there. `downloadCapture` saves one as a file, which the UI and the command line both open. Fragment inputs other than builtins are given with the `inputs` option, since the hook can't see a vertex stage's outputs.

WebGL2 works the same way, and can also follow one pixel through a frame.

```ts
import { installWebGL2, openInTexelscope } from '@texelscope/sdk';

const gl2 = installWebGL2();

// Every draw in the next frame that changed the pixel, counted from the top left.
const captures = await gl2.captureAt(canvas, x, y);
await openInTexelscope(captures[captures.length - 1], { pixel: [x, y] });
```

The WebGL2 hook reads each draw's uniforms, textures and samplers back from the GPU. It then runs the draw again with a shader that writes out the fragment inputs, which records them for every pixel of a window around the pixel, 128 pixels on a side by default.

## The command line

```sh
cargo run -p texelscope-cli -- check examples/scalar-field/field.wgsl
cargo run -p texelscope-cli -- emit examples/scalar-field/field.wgsl --to glsl-es
cargo run -p texelscope-cli -- capture examples/scalar-field/capture.json
cargo run -p texelscope-cli -- trace examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- explain examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- compare examples/scalar-field/capture.json --pixel 100,128 --with 140,128
cargo run -p texelscope-cli -- render examples/scalar-field/capture.json -o field.png
```

`trace`, `explain` and `compare` take `--json` for the full data and `--overrides` with a JSON file of pins and forced branches. `render` shades every pixel on the CPU, which gives a reference image to compare with the GPU.

## Not yet

Following a pixel through a frame in WebGPU, a Firefox build of the extension and a whole-frame comparison of the GPU and the interpreter are next.

## License

Texelscope is available under either the MIT license or the Apache License 2.0, at your option.
