// A small WebGL2 scene for trying Texelscope: lit boxes on a checkered floor, drawn into an
// offscreen target, then a full-screen pass that grades and vignettes the picture.

const canvas = document.getElementById('scene');
const gl = canvas.getContext('webgl2', { antialias: false });

const SCENE_VERTEX = `#version 300 es
in vec3 a_position;
in vec3 a_normal;
uniform mat4 u_model;
uniform mat4 u_view_projection;
out vec3 v_world;
out vec3 v_normal;
void main() {
  vec4 world = u_model * vec4(a_position, 1.0);
  v_world = world.xyz;
  v_normal = mat3(u_model) * a_normal;
  gl_Position = u_view_projection * world;
}`;

const SCENE_FRAGMENT = `#version 300 es
precision highp float;
uniform vec3 u_albedo;
uniform vec3 u_eye;
uniform vec3 u_light_dirs[2];
uniform vec3 u_light_colors[2];
uniform bool u_checker;
uniform bool u_rim;
uniform float u_fog_density;
in vec3 v_world;
in vec3 v_normal;
out vec4 color;

float checker(vec2 p) {
  vec2 cell = floor(p);
  return mod(cell.x + cell.y, 2.0);
}

void main() {
  vec3 n = normalize(v_normal);
  vec3 albedo = u_albedo;
  if (u_checker) {
    albedo *= mix(0.55, 1.0, checker(v_world.xz));
  }
  vec3 light = vec3(0.08);
  for (int i = 0; i < 2; i++) {
    float facing = max(dot(n, -u_light_dirs[i]), 0.0);
    light += u_light_colors[i] * facing;
  }
  vec3 shaded = albedo * light;
  vec3 view = normalize(u_eye - v_world);
  if (u_rim) {
    float rim = pow(1.0 - max(dot(n, view), 0.0), 3.0);
    shaded += vec3(0.25, 0.35, 0.5) * rim;
  }
  float distance_to_eye = length(u_eye - v_world);
  float fog = 1.0 - exp(-u_fog_density * distance_to_eye);
  color = vec4(mix(shaded, vec3(0.62, 0.7, 0.8), clamp(fog, 0.0, 1.0)), 1.0);
}`;

const POST_VERTEX = `#version 300 es
out vec2 v_uv;
void main() {
  vec2 corner = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  v_uv = corner;
  gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}`;

const POST_FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D u_scene;
uniform float u_exposure;
uniform bool u_grade;
in vec2 v_uv;
out vec4 color;
void main() {
  vec3 c = texture(u_scene, v_uv).rgb * u_exposure;
  if (u_grade) {
    c = c / (c + vec3(0.6)) * 1.6;
    c = mix(vec3(dot(c, vec3(0.299, 0.587, 0.114))), c, 1.15);
  }
  float vignette = smoothstep(0.95, 0.3, length(v_uv - 0.5));
  color = vec4(c * mix(0.7, 1.0, vignette), 1.0);
}`;

function program(vertex, fragment) {
  const p = gl.createProgram();
  for (const [type, source] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]]) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
    gl.attachShader(p, shader);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}

/** A box from -1 to 1 with a normal per face, as positions and normals interleaved. */
function box() {
  const faces = [
    [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
    [[0, 1, 0], [0, 0, 1], [1, 0, 0]],
    [[0, -1, 0], [0, 0, -1], [1, 0, 0]],
    [[0, 0, 1], [1, 0, 0], [0, 1, 0]],
    [[0, 0, -1], [-1, 0, 0], [0, 1, 0]],
  ];
  const data = [];
  for (const [n, u, v] of faces) {
    const corner = (a, b) => n.map((x, i) => x + a * u[i] + b * v[i]);
    for (const [a, b] of [[-1, -1], [1, -1], [1, 1], [-1, -1], [1, 1], [-1, 1]]) data.push(...corner(a, b), ...n);
  }
  return new Float32Array(data);
}

function mesh(p, data) {
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
  const position = gl.getAttribLocation(p, 'a_position');
  const normal = gl.getAttribLocation(p, 'a_normal');
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 3, gl.FLOAT, false, 24, 0);
  gl.enableVertexAttribArray(normal);
  gl.vertexAttribPointer(normal, 3, gl.FLOAT, false, 24, 12);
  gl.bindVertexArray(null);
  return { vao, count: data.length / 6 };
}

function perspective(fov, aspect, near, far) {
  const f = 1 / Math.tan(fov / 2);
  return [f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (far + near) / (near - far), -1, 0, 0, (2 * far * near) / (near - far), 0];
}

function lookAt(eye, target) {
  const sub = (a, b) => a.map((x, i) => x - b[i]);
  const norm = (a) => a.map((x) => x / Math.hypot(...a));
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const dot = (a, b) => a.reduce((s, x, i) => s + x * b[i], 0);
  const z = norm(sub(eye, target));
  const x = norm(cross([0, 1, 0], z));
  const y = cross(z, x);
  return [x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -dot(x, eye), -dot(y, eye), -dot(z, eye), 1];
}

function multiply(a, b) {
  const out = new Array(16).fill(0);
  for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) for (let k = 0; k < 4; k++) out[c * 4 + r] += a[k * 4 + r] * b[c * 4 + k];
  return out;
}

const model = (scale, at) => [scale[0], 0, 0, 0, 0, scale[1], 0, 0, 0, 0, scale[2], 0, at[0], at[1], at[2], 1];

const sceneProgram = program(SCENE_VERTEX, SCENE_FRAGMENT);
const postProgram = program(POST_VERTEX, POST_FRAGMENT);
const cube = mesh(sceneProgram, box());
const objects = [
  { scale: [8, 0.05, 8], at: [0, -0.05, 0], albedo: [0.8, 0.8, 0.78], checker: true },
  { scale: [0.6, 0.6, 0.6], at: [-1.2, 0.6, 0.3], albedo: [0.9, 0.35, 0.25], checker: false },
  { scale: [0.45, 0.9, 0.45], at: [1.1, 0.9, -0.4], albedo: [0.25, 0.55, 0.9], checker: false },
  { scale: [0.35, 0.35, 0.35], at: [0.2, 0.35, 1.3], albedo: [0.95, 0.8, 0.3], checker: false },
];

let sceneTarget;
let width = 0;
let height = 0;

/** The offscreen target the scene draws into, sized to the canvas. */
function resize() {
  const w = Math.max(1, Math.floor(canvas.clientWidth * devicePixelRatio));
  const h = Math.max(1, Math.floor(canvas.clientHeight * devicePixelRatio));
  if (w === width && h === height) return;
  width = canvas.width = w;
  height = canvas.height = h;
  const texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  const depth = gl.createRenderbuffer();
  gl.bindRenderbuffer(gl.RENDERBUFFER, depth);
  gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT24, w, h);
  const framebuffer = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0);
  gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, depth);
  sceneTarget = { texture, framebuffer };
}

const locations = (p, names) => Object.fromEntries(names.map((n) => [n, gl.getUniformLocation(p, n)]));
const sceneUniforms = locations(sceneProgram, ['u_model', 'u_view_projection', 'u_albedo', 'u_eye', 'u_light_dirs', 'u_light_colors', 'u_checker', 'u_rim', 'u_fog_density']);
const postUniforms = locations(postProgram, ['u_scene', 'u_exposure', 'u_grade']);
const postArray = gl.createVertexArray();

function frame(time) {
  resize();
  const angle = time * 0.0002;
  const eye = [Math.sin(angle) * 5.5, 2.6, Math.cos(angle) * 5.5];
  const viewProjection = multiply(perspective(0.9, width / height, 0.1, 40), lookAt(eye, [0, 0.4, 0]));

  gl.bindFramebuffer(gl.FRAMEBUFFER, sceneTarget.framebuffer);
  gl.viewport(0, 0, width, height);
  gl.clearColor(0.62, 0.7, 0.8, 1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  gl.enable(gl.DEPTH_TEST);
  gl.enable(gl.CULL_FACE);
  gl.useProgram(sceneProgram);
  gl.uniformMatrix4fv(sceneUniforms.u_view_projection, false, viewProjection);
  gl.uniform3fv(sceneUniforms.u_eye, eye);
  gl.uniform3fv(sceneUniforms.u_light_dirs, [-0.4, -1, -0.3, 0.8, -0.5, 0.6].map((v) => v / 1.2));
  gl.uniform3fv(sceneUniforms.u_light_colors, [1, 0.95, 0.85, 0.25, 0.3, 0.45]);
  gl.uniform1i(sceneUniforms.u_rim, 1);
  gl.uniform1f(sceneUniforms.u_fog_density, 0.045);
  gl.bindVertexArray(cube.vao);
  for (const o of objects) {
    gl.uniformMatrix4fv(sceneUniforms.u_model, false, model(o.scale, o.at));
    gl.uniform3fv(sceneUniforms.u_albedo, o.albedo);
    gl.uniform1i(sceneUniforms.u_checker, o.checker ? 1 : 0);
    gl.drawArrays(gl.TRIANGLES, 0, cube.count);
  }

  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  gl.disable(gl.DEPTH_TEST);
  gl.disable(gl.CULL_FACE);
  gl.useProgram(postProgram);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, sceneTarget.texture);
  gl.uniform1i(postUniforms.u_scene, 0);
  gl.uniform1f(postUniforms.u_exposure, 1.1);
  gl.uniform1i(postUniforms.u_grade, 1);
  gl.bindVertexArray(postArray);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  requestAnimationFrame(frame);
}

requestAnimationFrame(frame);
