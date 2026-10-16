// Programs the hook links itself, from an app program's vertex shader and another fragment
// shader, and the uniform values it gives them.

/** One uniform value, by the name WebGL reports for each array element. */
export interface UniformValue {
  name: string;
  type: number;
  value: unknown;
}

/**
 * Links `fragmentSource` with an app program's vertex shader. Attributes keep the locations
 * the app's program gave them, so its vertex arrays still fit. Returns the log on failure.
 */
export function linkWith(gl: WebGL2RenderingContext, app: WebGLProgram, vertex: WebGLShader, fragmentSource: string): WebGLProgram | string {
  const fragment = gl.createShader(gl.FRAGMENT_SHADER);
  const program = gl.createProgram();
  if (!fragment || !program) return gl.isContextLost() ? "the app's WebGL context was lost" : 'WebGL could not make a program';
  gl.shaderSource(fragment, fragmentSource);
  gl.compileShader(fragment);
  if (!gl.getShaderParameter(fragment, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(fragment) ?? '';
    gl.deleteShader(fragment);
    gl.deleteProgram(program);
    return log.trim() || 'the fragment shader did not compile';
  }
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  const attributes = gl.getProgramParameter(app, gl.ACTIVE_ATTRIBUTES) as number;
  for (let i = 0; i < attributes; i++) {
    const info = gl.getActiveAttrib(app, i);
    if (!info || info.name.startsWith('gl_')) continue;
    const location = gl.getAttribLocation(app, info.name);
    if (location >= 0) gl.bindAttribLocation(program, location, info.name);
  }
  gl.linkProgram(program);
  // The program keeps the shader for as long as it lives.
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(program) ?? '';
    gl.deleteProgram(program);
    return log.trim() || 'the program did not link';
  }
  return program;
}

/** Every uniform outside a block, one entry per array element, as getUniform reads them now. */
export function readUniforms(gl: WebGL2RenderingContext, program: WebGLProgram): UniformValue[] {
  const out: UniformValue[] = [];
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS) as number;
  for (let i = 0; i < count; i++) {
    const info = gl.getActiveUniform(program, i);
    if (!info) continue;
    const base = info.name.replace(/\[0\]$/, '');
    for (let k = 0; k < info.size; k++) {
      const name = info.size > 1 ? `${base}[${k}]` : info.name;
      const location = gl.getUniformLocation(program, name);
      // Members of blocks have no location, and live in the block's buffer.
      if (location) out.push({ name, type: info.type, value: gl.getUniform(program, location) });
    }
  }
  return out;
}

/** Gives the bound program the values for every uniform it shares with `values` by name and type. */
export function writeUniforms(gl: WebGL2RenderingContext, program: WebGLProgram, values: UniformValue[]) {
  const types = new Map<string, number>();
  const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS) as number;
  for (let i = 0; i < count; i++) {
    const info = gl.getActiveUniform(program, i);
    if (!info) continue;
    const base = info.name.replace(/\[0\]$/, '');
    for (let k = 0; k < info.size; k++) types.set(info.size > 1 ? `${base}[${k}]` : info.name, info.type);
  }
  for (const { name, type, value } of values) {
    // A uniform an edit retyped keeps its default rather than raise a GL error in the app.
    if (types.get(name) !== type) continue;
    const location = gl.getUniformLocation(program, name);
    if (location) setUniform(gl, location, type, value);
  }
}

/** Sets a uniform from the value getUniform gave for it. */
export function setUniform(gl: WebGL2RenderingContext, at: WebGLUniformLocation, type: number, value: unknown) {
  const floats = () => Float32Array.from(value as ArrayLike<number>);
  const ints = () => Int32Array.from(value as ArrayLike<number | boolean>, Number);
  const uints = () => Uint32Array.from(value as ArrayLike<number>);
  switch (type) {
    case gl.FLOAT: return gl.uniform1f(at, value as number);
    case gl.FLOAT_VEC2: return gl.uniform2fv(at, floats());
    case gl.FLOAT_VEC3: return gl.uniform3fv(at, floats());
    case gl.FLOAT_VEC4: return gl.uniform4fv(at, floats());
    case gl.INT_VEC2: case gl.BOOL_VEC2: return gl.uniform2iv(at, ints());
    case gl.INT_VEC3: case gl.BOOL_VEC3: return gl.uniform3iv(at, ints());
    case gl.INT_VEC4: case gl.BOOL_VEC4: return gl.uniform4iv(at, ints());
    case gl.UNSIGNED_INT: return gl.uniform1ui(at, value as number);
    case gl.UNSIGNED_INT_VEC2: return gl.uniform2uiv(at, uints());
    case gl.UNSIGNED_INT_VEC3: return gl.uniform3uiv(at, uints());
    case gl.UNSIGNED_INT_VEC4: return gl.uniform4uiv(at, uints());
    case gl.FLOAT_MAT2: return gl.uniformMatrix2fv(at, false, floats());
    case gl.FLOAT_MAT3: return gl.uniformMatrix3fv(at, false, floats());
    case gl.FLOAT_MAT4: return gl.uniformMatrix4fv(at, false, floats());
    case gl.FLOAT_MAT2x3: return gl.uniformMatrix2x3fv(at, false, floats());
    case gl.FLOAT_MAT2x4: return gl.uniformMatrix2x4fv(at, false, floats());
    case gl.FLOAT_MAT3x2: return gl.uniformMatrix3x2fv(at, false, floats());
    case gl.FLOAT_MAT3x4: return gl.uniformMatrix3x4fv(at, false, floats());
    case gl.FLOAT_MAT4x2: return gl.uniformMatrix4x2fv(at, false, floats());
    case gl.FLOAT_MAT4x3: return gl.uniformMatrix4x3fv(at, false, floats());
    // Ints, bools and samplers, whose value is a texture unit.
    default: return gl.uniform1i(at, Number(value));
  }
}

/** The texture target and binding query for each sampler type. */
export function samplerTarget(gl: WebGL2RenderingContext, type: number): [number, number] | undefined {
  switch (type) {
    case gl.SAMPLER_2D:
    case gl.INT_SAMPLER_2D:
    case gl.UNSIGNED_INT_SAMPLER_2D:
    case gl.SAMPLER_2D_SHADOW:
      return [gl.TEXTURE_2D, gl.TEXTURE_BINDING_2D];
    case gl.SAMPLER_3D:
    case gl.INT_SAMPLER_3D:
    case gl.UNSIGNED_INT_SAMPLER_3D:
      return [gl.TEXTURE_3D, gl.TEXTURE_BINDING_3D];
    case gl.SAMPLER_CUBE:
    case gl.INT_SAMPLER_CUBE:
    case gl.UNSIGNED_INT_SAMPLER_CUBE:
    case gl.SAMPLER_CUBE_SHADOW:
      return [gl.TEXTURE_CUBE_MAP, gl.TEXTURE_BINDING_CUBE_MAP];
    case gl.SAMPLER_2D_ARRAY:
    case gl.INT_SAMPLER_2D_ARRAY:
    case gl.UNSIGNED_INT_SAMPLER_2D_ARRAY:
    case gl.SAMPLER_2D_ARRAY_SHADOW:
      return [gl.TEXTURE_2D_ARRAY, gl.TEXTURE_BINDING_2D_ARRAY];
    default:
      return undefined;
  }
}
