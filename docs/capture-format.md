# Capture format

A capture holds everything Texelscope needs to replay one fragment workload. That means the shader, the color target, the fragment inputs and every bound resource. It is a JSON file, and large binary data can live in files next to it.

The format is at version 0 and will change without notice until the first release. The Rust types in `crates/texelscope-capture` are the reference, `packages/sdk/src/capture.ts` mirrors them, and `texelscope schema` prints a JSON Schema generated from the Rust side.

## Top level

| Field | Type | Meaning |
|---|---|---|
| `version` | number | Format version, currently 0 |
| `label` | string, optional | A name for people |
| `frame` | number, optional | Frame index in the app it came from |
| `shader` | object | Language, source and entry point |
| `target` | object | Width, height and format of the color target |
| `inputs` | object, optional | How fragment inputs are produced, `builtins` by default |
| `bindings` | array, optional | Resources by group and binding, or by name for GLSL |
| `reference` | object, optional | Pixels the app's GPU drew, for showing and comparing |

## Shader

`language` is `wgsl` or `glsl`. GLSL is GLSL ES 3.00 as WebGL2 takes it, and its entry point is `main`. `source` is either `{ "inline": "..." }` or `{ "path": "file.wgsl" }`, with paths relative to the capture file. `entryPoint` names the fragment entry point to replay.

## Target

`target` gives the `width`, `height` and `format` of the color target. `yUp` is true for captures from WebGL, which counts window y up from the bottom. Pixels are still numbered from the top in the capture and everywhere in Texelscope, and the flag flips the sign of derivatives in y as WebGL does.

## Fragment inputs

With `{ "kind": "builtins" }` the shader sees only builtins such as `position`.

With `{ "kind": "fullscreen", "locations": [...] }` each listed location is interpolated across the target from four corner values, given in the order top-left, top-right, bottom-left, bottom-right. The corners sit on the outer edges of the target and each pixel takes the value at its center. This covers full-screen passes, where inputs vary linearly across the screen.

With `{ "kind": "window", "rect": {...}, "fragCoord": "...", "varyings": [...] }` the inputs were read back from the GPU at every pixel of a window of the target, which `rect` places with `x`, `y`, `width` and `height`. `fragCoord` holds the fragment position at each pixel as four floats. Each varying gives the `name` the shader declares, the number of floats per pixel in `components`, and `data`. Both are base64 of little-endian 32-bit floats with rows from the top, and a pixel the draw didn't cover holds NaN. Only pixels inside the window can be explained.

## Bindings

Each binding has `group`, `binding`, an optional `label` and a `resource` whose `kind` is `uniform`, `storage`, `texture` or `sampler`.

WebGL binds by name, so a GLSL capture gives each binding the `name` of the uniform, uniform block or sampler it binds, and Texelscope matches bindings by that name and their kind. A uniform declared on its own, such as `uniform float u_scale`, is a `uniform` binding named after it whose values hold that one member. A `sampler2D` comes as a `texture` and a `sampler` binding that share its name.

Uniform and storage buffers carry `data` in one of three forms. `{ "values": { ... } }` gives member values by name, and Texelscope packs them with the layout the shader declares. `{ "base64": "..." }` and `{ "path": "file.bin" }` give the raw bytes the shader sees, after any binding offset and dynamic offset.

Textures give `dimension` (`1d`, `2d` or `3d`), `format` as a WebGPU format name, `size` as width, height and depth or layer count, an optional `mipLevelCount` and `data`. Inline data lists texel components as the shader reads them, all mip levels in order and rows from the top. Raw bytes use `base64` or `path` as buffers do, with no row padding.

Samplers use the WebGPU field names `addressModeU`, `addressModeV`, `addressModeW`, `magFilter`, `minFilter` and `mipmapFilter`, with WebGPU's defaults.

## Reference

`reference` holds what the app's GPU drew, so the UI can show it and compare it with the interpreter. `rect` places the pixels in the target, and `rgba8` is base64 of one byte per channel with rows from the top.

## Numbers

JSON cannot hold NaN or infinities. Traces and other output that may hold them write the strings `"NaN"`, `"Infinity"` and `"-Infinity"` instead. Inline texture data must be finite, so use `base64` or a file when it is not.

## Example

`examples/scalar-field/capture.json` replays a full-screen pass over a synthetic 8 by 8 field.
