# Texelscope

Texelscope is a debugger for WGSL and GLSL shaders that explains why a pixel has its color. It replays a captured draw on a reference interpreter and on the GPU. You can follow any pixel or region from the data it reads to the color it writes, read an account of what decided the result, and try changes to the shader while it runs.

Texelscope is at the scaffold stage. Today it parses and validates WGSL, translates it to GLSL ES 3.00 and loads capture files. The interpreter, explanations, overrides, GPU probes and the in-page hooks come next.

## What it will do

- Trace one pixel step by step, forward and back, with every value shown next to the source that produced it.
- Turn any expression into an image over a region, with a histogram and a map of the branches each pixel took.
- Explain a pixel or a region as a chain from inputs to color, including how close each branch came to going the other way and which inputs move the color most.
- Apply overrides without editing the source. Pin a value, force a branch, replace an expression or scrub a constant, for the whole frame or one region, and compare the result with the original.
- Run the same kernel as WGSL on WebGPU, as GLSL on WebGL2 and on the interpreter, then point to the first expression where the results differ.
- Attach to a running page through a small SDK for WebGPU and WebGL2, including renderers that live in web workers.

## Layout

| Path | Contents |
|---|---|
| `crates/texelscope-capture` | The capture format, loading and validation |
| `crates/texelscope-ir` | WGSL parsing and validation through naga, expression ids and values |
| `crates/texelscope-interp` | The reference interpreter and its trace format |
| `crates/texelscope-explain` | Explanations built from traces |
| `crates/texelscope-rewrite` | Code emission, what-if overrides and GPU probes |
| `crates/texelscope-cli` | The `texelscope` command |
| `crates/texelscope-wasm` | Bindings for the browser |
| `packages/sdk` | In-page hooks for WebGPU and WebGL2 |
| `packages/ui` | The web UI, built from Lit web components |
| `examples` | Synthetic shaders and captures |
| `docs` | The capture format |

## Building

Texelscope needs Rust 1.87 or newer and Node 20.19 or newer.

```sh
cargo test --workspace
cargo run -p texelscope-cli -- check examples/scalar-field/field.wgsl
cargo run -p texelscope-cli -- emit examples/scalar-field/field.wgsl --to glsl-es
cargo run -p texelscope-cli -- capture examples/scalar-field/capture.json
npm install
npm run dev
```

`npm run build:wasm` builds the browser bindings with wasm-pack.

## License

Texelscope is available under either the MIT license or the Apache License 2.0, at your option.
