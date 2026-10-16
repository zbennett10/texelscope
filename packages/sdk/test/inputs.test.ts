import { describe, expect, it } from 'vitest';
import { declaredInputs } from '../src/webgl2/inputs.js';

describe('declaredInputs', () => {
  it('finds every input, including ones declared back to back', () => {
    const inputs = declaredInputs(`#version 300 es
precision highp float;
uniform vec3 u_tint;
flat in float v_id;
in vec2 v_uv;
layout(location = 2) in highp vec4 v_extra;
// in vec3 v_commented;
out vec4 color;
void main() { color = vec4(v_uv, v_id, v_extra.x); }`);
    expect(inputs.map((i) => [i.name, i.components, i.declaration])).toEqual([
      ['v_id', 1, 'flat in float v_id;'],
      ['v_uv', 2, 'in vec2 v_uv;'],
      ['v_extra', 4, 'in highp vec4 v_extra;'],
    ]);
  });
});
