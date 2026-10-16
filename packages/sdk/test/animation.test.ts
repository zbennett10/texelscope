import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/** A browser's frame loop: callbacks wait until the test renders a frame. */
function frameLoop() {
  let next = 1;
  const queued = new Map<number, FrameRequestCallback>();
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    queued.set(next, callback);
    return next++;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => queued.delete(id));
  return {
    /** Runs every callback asked for before this frame, then the timers set during it. */
    render(time: number) {
      const callbacks = [...queued.values()];
      queued.clear();
      for (const callback of callbacks) callback(time);
      vi.advanceTimersByTime(0);
    },
  };
}

/** A fresh copy of the module, since it keeps its counts for the page's lifetime. */
const load = () => import('../src/animation.js');

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe('animation', () => {
  it('counts the callbacks of one render as one frame and ends it after the last', async () => {
    const loop = frameLoop();
    const { animation, useAnimation } = await load();
    const release = useAnimation();
    const order: string[] = [];
    requestAnimationFrame(() => {
      order.push('a');
      animation.afterFrame(() => order.push('end'));
    });
    requestAnimationFrame(() => order.push('b'));
    loop.render(16);
    expect(order).toEqual(['a', 'b', 'end']);
    expect(animation.count).toBe(1);
    release();
  });

  it('ends a draw made outside the callbacks at the next frame', async () => {
    const loop = frameLoop();
    const { animation, useAnimation } = await load();
    const release = useAnimation();
    let ended = false;
    animation.drew();
    animation.afterFrame(() => (ended = true));
    vi.advanceTimersByTime(0);
    expect(ended).toBe(false);
    loop.render(16);
    expect(ended).toBe(true);
    release();
  });

  it('draws the last drawing frame again only when the app has no frame coming', async () => {
    const loop = frameLoop();
    const { animation, useAnimation } = await load();
    const release = useAnimation();
    const times: number[] = [];
    requestAnimationFrame((time) => {
      times.push(time);
      animation.drew();
    });
    loop.render(16);
    const coming = requestAnimationFrame(() => {});
    expect(animation.redraw()).toBe(false);
    cancelAnimationFrame(coming);
    expect(animation.redraw()).toBe(true);
    loop.render(32);
    expect(times).toEqual([16, 16]);
    expect(animation.count).toBe(2);
    release();
  });

  it('draws the last drawing frame again once, however often it is asked before the next frame', async () => {
    const loop = frameLoop();
    const { animation, useAnimation } = await load();
    const release = useAnimation();
    const times: number[] = [];
    requestAnimationFrame((time) => {
      times.push(time);
      animation.drew();
    });
    loop.render(16);
    expect(animation.redraw()).toBe(true);
    expect(animation.redraw()).toBe(true);
    loop.render(32);
    expect(times).toEqual([16, 16]);
    release();
  });

  it('has nothing to draw again until a frame draws', async () => {
    const loop = frameLoop();
    const { animation, useAnimation } = await load();
    const release = useAnimation();
    requestAnimationFrame(() => {});
    loop.render(16);
    expect(animation.redraw()).toBe(false);
    release();
  });

  it('puts the frame functions back when the last hook lets go', async () => {
    frameLoop();
    const original = globalThis.requestAnimationFrame;
    const { useAnimation } = await load();
    const first = useAnimation();
    const second = useAnimation();
    first();
    first();
    expect(globalThis.requestAnimationFrame).not.toBe(original);
    second();
    expect(globalThis.requestAnimationFrame).toBe(original);
  });
});
