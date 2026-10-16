// The WebGL2 hook in a real browser: what it keeps of a draw lets the page draw it again, and
// the app's own state and frames stay as they were.

import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { type Browser, chromePath, openPage } from './browser/chrome.js';

describe.skipIf(!chromePath)('the WebGL2 hook in Chrome', () => {
  let browser: Browser;
  beforeAll(async () => {
    browser = await openPage('webgl2.html');
  }, 60000);
  afterAll(async () => {
    await browser?.close();
  });
  const scene = <T>(name: string) => browser.evaluate<T>(`scenes.${name}()`);

  it('records the inputs of the fragment nearest the eye where one draw overlaps itself', async () => {
    const { app, id } = await scene<{ app: number[]; id: number[] }>('nearestInputs');
    // The near triangle has id 1, which the app's pixel shows in green.
    expect(app[1]).toBe(64);
    expect(id).toEqual([1]);
  });

  it('draws a kept draw again as the app drew it', async () => {
    const { worst, middle } = await scene<{ worst: number; middle: number[] }>('replayed');
    expect(worst).toBeLessThanOrEqual(1);
    expect(middle[1]).toBeCloseTo(0.25, 5);
  });

  it('draws an edit with the uniforms the draw had, and reports what fails to compile', async () => {
    const { middle, broken } = await scene<{ middle: number[]; broken: string }>('edited');
    expect(middle).toEqual([1, 0.5, 0.25, 0.75]);
    expect(broken).toMatch(/nope/);
  });

  it("leaves the app's state as it was", async () => {
    const { replayed, recorded, changed, error } = await scene<{ replayed: boolean; recorded: boolean; changed: string[]; error: number }>('stateKept');
    expect(replayed && recorded).toBe(true);
    expect(changed).toEqual([]);
    expect(error).toBe(0);
  });

  it('samples a render target as it was when the draw was captured', async () => {
    expect(await scene<number[]>('renderTarget')).toEqual([1, 0, 0, 1]);
  });

  it("runs an edit in the app's own draws, with the uniforms the app sets", async () => {
    const { failed, edited, back, error } = await scene<{ failed: string | null; edited: number[]; back: number[]; error: number }>('swapped');
    expect(failed).toBeNull();
    expect(edited).toEqual([0, 255, 0, 255]);
    // The app's own shader, back in place: green is v_id / 4 for the near triangle.
    expect(back[1]).toBe(64);
    expect(error).toBe(0);
  });

  it('answers the Texelscope window about a draw, and no other window', async () => {
    const r = await scene<{
      live: { page: string; draw: number } | null;
      replayed: { width: number; middle: number[] } | null;
      window: unknown;
      reloaded: string | null;
      stranger: unknown;
      scene: boolean;
      frame: number[] | null;
      back: number[];
    }>('liveChannel');
    expect(r.live?.draw).toBe(1);
    expect(r.replayed?.width).toBe(32);
    expect(r.replayed?.middle[1]).toBeCloseTo(0.25, 5);
    expect(r.window).toEqual({ x: 0, y: 0, width: 8, height: 8 });
    expect(r.reloaded).toMatch(/reloaded/);
    expect(r.stranger).toBeNull();
    expect(r.scene).toBe(true);
    expect(r.frame).toEqual([0, 0, 255, 255]);
    expect(r.back[2]).not.toBe(255);
  });

  it('shows each new edit, and then its own shader, in an app that draws only when something changes', async () => {
    const { blue, red, back } = await scene<{ blue: boolean; red: boolean; back: boolean }>('idleEdits');
    expect(blue).toBe(true);
    expect(red).toBe(true);
    expect(back).toBe(true);
  });

  it('records inputs around another pixel later, from the vertex arrays the draw had', async () => {
    const { uv, id } = await scene<{ uv: number[]; id: number[] }>('movedWindow');
    expect(uv[0]).toBeCloseTo(28.5 / 32, 5);
    expect(uv[1]).toBeCloseTo(1 - 28.5 / 32, 5);
    expect(id).toEqual([1]);
  });
});
