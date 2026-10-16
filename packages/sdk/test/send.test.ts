import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Capture } from '../src/capture.js';

type Listener = (event: { source: unknown; data: unknown; origin: string }) => void;

/** A page with `window.open`, and the tab it opens. */
function browser(options: { loaded: boolean; answers: boolean }) {
  const listeners = new Set<Listener>();
  const posted: { message: { type: string }; origin: string }[] = [];
  const tab = {
    closed: false,
    focus: vi.fn(),
    href: options.loaded ? 'http://localhost:5173/' : 'about:blank',
    get location() {
      // A loaded UI tab is on another origin, so reading its location throws.
      if (options.loaded) throw new DOMException('cross-origin', 'SecurityError');
      const tab = this;
      return {
        get href() {
          return tab.href;
        },
        set href(value: string) {
          tab.href = value;
          if (options.answers) queueMicrotask(() => say({ type: 'texelscope:ready' }));
        },
      };
    },
    postMessage(message: { type: string }, origin: string) {
      posted.push({ message, origin });
      // Browsers drop a message whose target origin isn't the tab's.
      const delivered = origin === '*' || origin === new URL(tab.href).origin;
      if (delivered && message.type === 'texelscope:hello' && options.answers) queueMicrotask(() => say({ type: 'texelscope:ready' }));
    },
  };
  const say = (data: unknown) => listeners.forEach((l) => l({ source: tab, data, origin: new URL(tab.href).origin }));
  const open = vi.fn(() => tab);
  vi.stubGlobal('window', {
    open,
    addEventListener: (_: string, l: Listener) => listeners.add(l),
    removeEventListener: (_: string, l: Listener) => listeners.delete(l),
  });
  return { tab, open, posted };
}

const capture = { version: 0 } as Capture;

afterEach(() => vi.unstubAllGlobals());

describe('openInTexelscope', () => {
  it('opens the UI, waits for it to listen, then sends the capture', async () => {
    const { openInTexelscope } = await import('../src/send.js');
    const { tab, open, posted } = browser({ loaded: false, answers: true });
    await openInTexelscope(capture, { pixel: [3, 4] });
    expect(open).toHaveBeenCalledWith('', 'texelscope', 'popup');
    expect(tab.href).toBe('http://localhost:5173/');
    expect(posted).toEqual([
      { message: { type: 'texelscope:capture', capture, pixel: [3, 4] }, origin: 'http://localhost:5173' },
    ]);
    expect(tab.focus).toHaveBeenCalled();
    tab.closed = true;
  });

  it('finds a UI tab that is already open without reloading it', async () => {
    const { openInTexelscope } = await import('../src/send.js');
    const { tab, posted } = browser({ loaded: true, answers: true });
    await openInTexelscope(capture, { focus: false });
    expect(tab.href).toBe('http://localhost:5173/');
    expect(posted.map((p) => p.message.type)).toEqual(['texelscope:hello', 'texelscope:capture']);
    expect(tab.focus).not.toHaveBeenCalled();
    tab.closed = true;
  });

  it('sends to the UI the Vite plugin or extension set for the page when a call names none', async () => {
    const { openInTexelscope, setDefaultUi } = await import('../src/send.js');
    const { tab, posted } = browser({ loaded: false, answers: true });
    setDefaultUi(() => 'http://localhost:5180/__texelscope/');
    await openInTexelscope(capture);
    delete (globalThis as Record<symbol, unknown>)[Symbol.for('texelscope.ui')];
    expect(tab.href).toBe('http://localhost:5180/__texelscope/');
    expect(posted[0]?.origin).toBe('http://localhost:5180');
    tab.closed = true;
  });

  it('sends to a Texelscope tab left open at another address than the one asked for', async () => {
    const { openInTexelscope } = await import('../src/send.js');
    const { tab, posted } = browser({ loaded: true, answers: true });
    await openInTexelscope(capture, { url: 'http://localhost:5180/__texelscope/', timeout: 50 });
    expect(tab.href).toBe('http://localhost:5173/');
    expect(posted.at(-1)).toEqual({ message: { type: 'texelscope:capture', capture, pixel: undefined }, origin: 'http://localhost:5173' });
    tab.closed = true;
  });

  it('gives up with a hint when the UI never answers', async () => {
    const { openInTexelscope } = await import('../src/send.js');
    browser({ loaded: false, answers: false });
    await expect(openInTexelscope(capture, { timeout: 20 })).rejects.toThrow(/did not answer at http:\/\/localhost:5173\/.*Is its UI running/);
  });
});
