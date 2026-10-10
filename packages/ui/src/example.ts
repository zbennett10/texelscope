import type { Capture } from '@texelscope/sdk';
import capture from '../../../examples/scalar-field/capture.json';
import field from '../../../examples/scalar-field/field.wgsl?raw';

/** The scalar-field example with its shader inlined, as the browser needs it. */
export function exampleCapture(): Capture {
  const example = structuredClone(capture) as unknown as Capture;
  example.shader.source = { inline: field };
  return example;
}
