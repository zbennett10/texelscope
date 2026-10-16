import { describe, expect, it } from 'vitest';
import { inspectPixel } from '../src/inspect.js';

describe('inspectPixel', () => {
  it('reads the GPU color from a frame that arrived while the CPU explained', async () => {
    let shown = [0, 1, 0, 1];
    let explained!: (why: string) => void;
    const pending = inspectPixel(
      () => new Promise<string>((resolve) => (explained = resolve)),
      async () => 'trace',
      async () => shown,
    );
    // A Live frame with the newest edit shows before the explanation is done.
    shown = [0, 0, 1, 1];
    explained('why');
    expect(await pending).toEqual(['why', 'trace', [0, 0, 1, 1]]);
  });
});
