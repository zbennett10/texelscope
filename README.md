# Texelscope

Texelscope is a debugger for WGSL and GLSL shaders that explains why a pixel has its color. It replays a captured draw on a reference interpreter and on the GPU. You can follow any pixel or region from the data it reads to the color it writes, read an account of what decided the result, and try changes to the shader while it runs.

## What works today

A small SDK captures a draw from a running WebGPU page, with its shader, its uniforms and every texel of its textures. The web UI replays the capture on the GPU and runs the same fragment shader on a CPU interpreter, so for any pixel you get the following.

- The color the GPU wrote next to the color the interpreter computed, and whether they agree.
- The chain from data to color. Each fragment input, uniform, texel, `let` and `var` the color depends on appears with its value and the code that produced it, in the order the shader computed them.
- Every decision on the way, with how far its operands were from flipping it, such as "false, 13.2 from flipping".
- The inputs that move the color most, measured by nudging each one and running the pixel again.
- A full trace you can step through. Each step's value is shown beside the source that produced it, and the values of named lets appear inline in the editor.

Shift-drag over the image and every named value and decision across that region appears as an image, in the order a pixel computes them. That shows the mapping from data to pixel one stage at a time, without debug modes in the shader.

To try a change, edit the source, pin any value or flip any decision. The GPU replay and the interpreter both pick it up, so you see the new image and the new explanation together. Captures arrive as frames, and the arrow keys step through them, so a pixel can be followed over time.

The interpreter handles WGSL fragment shaders that use scalars, vectors, matrices, structs, arrays, pointers, loops, switches, function calls, every math builtin, `textureLoad`, filtered and mipmapped `textureSample` variants, and derivatives. It runs the four pixels of a quad together so `dpdx`, `dpdy` and `fwidth` match the GPU.

## Layout

| Path | Contents |
|---|---|
| `crates/texelscope-capture` | The capture format, loading, validation and texel decoding |
| `crates/texelscope-ir` | WGSL parsing and validation through naga, expression ids and values |
| `crates/texelscope-interp` | The reference interpreter, its trace format and region inspection |
| `crates/texelscope-explain` | Explanations built from traces |
| `crates/texelscope-rewrite` | Code emission, what-if overrides and GPU probes |
| `crates/texelscope-cli` | The `texelscope` command |
| `crates/texelscope-wasm` | Bindings for the browser |
| `packages/sdk` | In-page hooks for WebGPU and WebGL2 |
| `packages/ui` | The web UI, built from Lit web components |
| `examples` | Synthetic shaders and captures |
| `docs` | The capture format |

## Building

Texelscope needs Rust 1.87 or newer, wasm-pack and Node 20.19 or newer.

```sh
cargo test --workspace
npm install
npm run dev
```

`npm run dev` builds the browser bindings and serves the UI at http://localhost:5173. Press Example to load the scalar-field capture.

## Debugging a WebGPU page

Install the hook before the page asks for its GPU device, so it sees every resource the app creates. Then capture a draw and send it to the UI.

```ts
import { installWebGPU, openInTexelscope } from '@texelscope/sdk';

const texelscope = installWebGPU({ filter: (pipeline) => pipeline.label === 'my-pass' });

// Later, for example from a key handler.
const capture = await texelscope.captureNextDraw();
await openInTexelscope(capture, { pixel: [x, y] });
```

The hook patches WebGPU's prototypes, so it works in a page or a worker. Captures from a worker can be posted to the page and sent on from there. `downloadCapture` saves one as a file, which the UI and the command line both open. Fragment inputs other than builtins are given with the `inputs` option, since the hook can't see a vertex stage's outputs.

## The command line

```sh
cargo run -p texelscope-cli -- check examples/scalar-field/field.wgsl
cargo run -p texelscope-cli -- emit examples/scalar-field/field.wgsl --to glsl-es
cargo run -p texelscope-cli -- capture examples/scalar-field/capture.json
cargo run -p texelscope-cli -- trace examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- explain examples/scalar-field/capture.json --pixel 100,128
cargo run -p texelscope-cli -- render examples/scalar-field/capture.json -o field.png
```

`trace` and `explain` take `--json` for the full data and `--overrides` with a JSON file of pins and forced branches. `render` shades every pixel on the CPU, which gives a reference image to compare with the GPU.

## Not yet

WebGL2 capture and hand-written GLSL, GPU probes that turn any expression into a full-frame image, and a whole-frame comparison of the GPU and the interpreter are next.

## License

Texelscope is available under either the MIT license or the Apache License 2.0, at your option.
