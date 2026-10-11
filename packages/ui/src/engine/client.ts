import type { Comparison, Description, Explanation, ExprRef, Override, Rect, RegionData, Trace } from '../types.js';

/** Talks to the core in its worker. Calls run one at a time, in order. */
export class Engine {
  #worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
  #pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  #next = 1;

  constructor() {
    this.#worker.onmessage = (event: MessageEvent<{ id: number; ok: boolean; result?: unknown; error?: string }>) => {
      const { id, ok, result, error } = event.data;
      const pending = this.#pending.get(id);
      this.#pending.delete(id);
      if (ok) pending?.resolve(result);
      else pending?.reject(new Error(error));
    };
  }

  #call<T>(method: string, ...args: unknown[]): Promise<T> {
    const id = this.#next++;
    return new Promise<T>((resolve, reject) => {
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
      this.#worker.postMessage({ id, method, args });
    });
  }

  load(captureJson: string) {
    return this.#call<Description>('load', captureJson);
  }
  setSource(source: string) {
    return this.#call<Description>('setSource', source);
  }
  trace(x: number, y: number, overrides: Override[]) {
    return this.#call<Trace>('trace', x, y, overrides);
  }
  explain(x: number, y: number, overrides: Override[]) {
    return this.#call<Explanation>('explain', x, y, overrides);
  }
  probe(expr: ExprRef, overrides: Override[]) {
    return this.#call<{ wgsl: string; components: number }>('probe', expr, overrides);
  }
  compare(a: [number, number], b: [number, number], overrides: Override[]) {
    return this.#call<Comparison>('compare', a, b, overrides);
  }
  region(rect: Rect, overrides: Override[], scope: Rect | null) {
    return this.#call<RegionData>('region', rect, overrides, scope);
  }
  patched(overrides: Override[]) {
    return this.#call<string>('patched', overrides);
  }
  bufferBytes(group: number, binding: number) {
    return this.#call<Uint8Array>('bufferBytes', group, binding);
  }
  textureBytes(group: number, binding: number) {
    return this.#call<Uint8Array>('textureBytes', group, binding);
  }
}
