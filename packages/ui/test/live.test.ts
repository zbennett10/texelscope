import { afterEach, describe, expect, it, vi } from 'vitest';
import { LivePage } from '../src/live.js';

/** A page window that keeps what Texelscope posts and answers when told. */
function page() {
  const posted: { message: Record<string, unknown>; origin: string }[] = [];
  const window = { postMessage: (message: Record<string, unknown>, origin: string) => posted.push({ message, origin }) } as unknown as Window;
  const live = new LivePage(window, 'http://app.test');
  const answer = (id: number, reply: Record<string, unknown> = {}) => live.receive({ type: 'texelscope:replay-done', id, ...reply });
  return { live, posted, answer };
}

const draw = { page: 'p', draw: 7 };

afterEach(() => vi.useRealTimers());

describe('LivePage', () => {
  it("asks the app's origin and takes the reply with the same id", async () => {
    const { live, posted, answer } = page();
    const pending = live.replay(draw, 'src', 'color');
    expect(posted).toEqual([{ message: { type: 'texelscope:replay', page: 'p', draw: 7, source: 'src', id: 1 }, origin: 'http://app.test' }]);
    const data = new Float32Array(4);
    expect(answer(1, { width: 1, height: 1, data })).toBe(true);
    expect(await pending).toEqual({ width: 1, height: 1, data });
  });

  it('runs only the newest request waiting in a lane, and the replaced ones come back null', async () => {
    const { live, posted, answer } = page();
    const first = live.replay(draw, 'a', 'color');
    const second = live.replay(draw, 'b', 'color');
    const third = live.replay(draw, 'c', 'color');
    // Another lane doesn't wait for this one.
    void live.replay(draw, 'probe', 'probe');
    expect(posted.map((p) => p.message.source)).toEqual(['a', 'probe']);
    expect(await second).toBeNull();
    answer(1, { error: 'it failed' });
    expect(await first).toBe('it failed');
    await vi.waitFor(() => expect(posted.map((p) => p.message.source)).toEqual(['a', 'probe', 'c']));
    answer(3, { width: 2, height: 1, data: new Float32Array(8) });
    expect(await third).toMatchObject({ width: 2 });
  });

  it("says the page didn't answer after a while", async () => {
    vi.useFakeTimers();
    const { live } = page();
    const pending = live.inputs(draw, [1, 2]);
    vi.advanceTimersByTime(15000);
    expect(await pending).toMatch(/didn't answer/);
  });

  it('leaves messages that are not replies alone', () => {
    const { live } = page();
    expect(live.receive({ type: 'texelscope:frame', page: 'p', draw: 7 })).toBe(false);
    expect(live.receive({ type: 'texelscope:capture' })).toBe(false);
  });
});
