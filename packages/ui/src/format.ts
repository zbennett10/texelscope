import type { Value } from './types.js';

/** A number as JSON carries it, including NaN and infinities spelled as strings. */
export function num(value: number | string): number {
  return typeof value === 'number' ? value : Number(value);
}

/** Up to four decimals, without trailing zeros. */
export function fixed(value: number, digits = 4): string {
  if (!Number.isFinite(value)) return String(value);
  const text = value.toFixed(digits).replace(/\.?0+$/, '');
  return text === '-0' ? '0' : text;
}

/** A value spelled roughly as WGSL would. */
export function formatValue(value: Value): string {
  if ('f32' in value) return fixed(num(value.f32));
  if ('i32' in value) return `${value.i32}i`;
  if ('u32' in value) return `${value.u32}u`;
  if ('bool' in value) return String(value.bool);
  if ('vector' in value) return `vec${value.vector.length}(${value.vector.map(formatValue).join(', ')})`;
  if ('matrix' in value) return `mat(${value.matrix.map(formatValue).join(', ')})`;
  if ('array' in value) return `array(${value.array.map(formatValue).join(', ')})`;
  return `{ ${value.struct.map(([k, v]) => `${k}: ${formatValue(v)}`).join(', ')} }`;
}

/** The numbers inside a scalar or vector. */
export function numbers(value: Value): number[] {
  if ('f32' in value) return [num(value.f32)];
  if ('i32' in value) return [value.i32];
  if ('u32' in value) return [value.u32];
  if ('bool' in value) return [value.bool ? 1 : 0];
  if ('vector' in value) return value.vector.flatMap(numbers);
  return [];
}

/** Rebuilds a value of the same shape from edited text such as `0.5` or `1, 2, 3`. */
export function parseLike(template: Value, text: string): Value | undefined {
  const parts = text.split(/[\s,()]+/).filter((p) => p !== '' && !/^vec\d$/.test(p));
  const values = parts.map(Number);
  if (values.some(Number.isNaN)) return undefined;
  const scalar = (like: Value, n: number): Value | undefined => {
    if ('f32' in like) return { f32: n };
    if ('i32' in like) return { i32: Math.trunc(n) };
    if ('u32' in like) return { u32: Math.max(0, Math.trunc(n)) };
    if ('bool' in like) return { bool: n !== 0 };
    return undefined;
  };
  if ('vector' in template) {
    if (values.length !== template.vector.length) return undefined;
    const lanes = template.vector.map((lane, i) => scalar(lane, values[i]!));
    return lanes.every(Boolean) ? { vector: lanes as Value[] } : undefined;
  }
  return values.length === 1 ? scalar(template, values[0]!) : undefined;
}

/** RGBA in 0..1 as a CSS color. */
export function css(rgba: number[]): string {
  const c = (v: number | undefined) => Math.round(Math.min(1, Math.max(0, v ?? 0)) * 255);
  return `rgba(${c(rgba[0])}, ${c(rgba[1])}, ${c(rgba[2])}, ${Math.min(1, Math.max(0, rgba[3] ?? 1))})`;
}

/** A perceptually even ramp for value images, from dark blue to yellow. */
export function ramp(t: number): [number, number, number] {
  const x = Math.min(1, Math.max(0, t));
  const r = 0.28 + x * (2.2 + x * (-6.7 + x * 5.2));
  const g = 0.0 + x * (1.4 + x * (-0.6 + x * 0.1));
  const b = 0.33 + x * (1.4 + x * (-4.0 + x * 2.4));
  return [Math.min(1, Math.max(0, r)), Math.min(1, Math.max(0, g)), Math.min(1, Math.max(0, b))];
}
