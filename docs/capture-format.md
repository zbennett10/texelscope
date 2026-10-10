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
| `bindings` | array, optional | Resources by group and binding |

## Shader

`language` is `wgsl` or `glsl`, and only WGSL is replayed today. `source` is either `{ "inline": "..." }` or `{ "path": "file.wgsl" }`, with paths relative to the capture file. `entryPoint` names the fragment entry point to replay.

## Fragment inputs

With `{ "kind": "builtins" }` the shader sees only builtins such as `position`.

With `{ "kind": "fullscreen", "locations": [...] }` each listed location is interpolated across the target from four corner values, given in the order top-left, top-right, bottom-left, bottom-right. The corners sit on the outer edges of the target and each pixel takes the value at its center. This covers full-screen passes, where inputs vary linearly across the screen.

Inputs captured per pixel from a running app will come later.

## Bindings

Each binding has `group`, `binding`, an optional `label` and a `resource` whose `kind` is `uniform`, `storage`, `texture` or `sampler`.

Uniform and storage buffers carry `data` in one of three forms. `{ "values": { ... } }` gives member values by name, and Texelscope packs them with the layout the shader declares. `{ "base64": "..." }` and `{ "path": "file.bin" }` give the raw bytes the shader sees, after any binding offset and dynamic offset.

Textures give `dimension` (`1d`, `2d` or `3d`), `format` as a WebGPU format name, `size` as width, height and depth or layer count, an optional `mipLevelCount` and `data`. Inline data lists texel components as the shader reads them, all mip levels in order and rows from the top. Raw bytes use `base64` or `path` as buffers do, with no row padding.

Samplers use the WebGPU field names `addressModeU`, `addressModeV`, `addressModeW`, `magFilter`, `minFilter` and `mipmapFilter`, with WebGPU's defaults.

## Numbers

JSON cannot hold NaN or infinities. Traces and other output that may hold them write the strings `"NaN"`, `"Infinity"` and `"-Infinity"` instead. Inline texture data must be finite, so use `base64` or a file when it is not.

## Example

`examples/scalar-field/capture.json` replays a full-screen pass over a synthetic 8 by 8 field.
