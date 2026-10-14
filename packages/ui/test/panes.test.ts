import { describe, expect, it } from 'vitest';
import { MIN_WIDTH, dragTo, paneWidths, parsePanes } from '../src/panes.js';
import { parseTheme } from '../src/prefs.js';

describe('pane widths', () => {
  it('scales the defaults with the window, within bounds', () => {
    expect(paneWidths({}, 1600)).toEqual({ left: 512, right: 416 });
    expect(paneWidths({}, 3000)).toEqual({ left: 640, right: 460 });
  });

  it('keeps the widths chosen by dragging when they fit', () => {
    expect(paneWidths({ left: 700, right: 300 }, 1600)).toEqual({ left: 700, right: 300 });
    expect(paneWidths({ left: 700 }, 1600)).toEqual({ left: 700, right: 416 });
  });

  it('shrinks both side panes in proportion when the window is too narrow for them', () => {
    // 360 too wide, taken from the 400 and 200 pixels the panes have above their minimums.
    expect(paneWidths({ left: 620, right: 480 }, 1000)).toEqual({ left: 380, right: 360 });
  });

  it('never goes below the minimums', () => {
    expect(paneWidths({ left: 50, right: 50 }, 400)).toEqual({ left: MIN_WIDTH.left, right: MIN_WIDTH.right });
  });
});

describe('dragging an edge', () => {
  it('follows the pointer', () => {
    expect(dragTo('left', 600, 1600, 400)).toBe(600);
    expect(dragTo('right', 1100, 1600, 500)).toBe(500);
  });

  it('stops at the minimum and where the image would get too narrow', () => {
    expect(dragTo('left', 10, 1600, 400)).toBe(MIN_WIDTH.left);
    expect(dragTo('left', 1500, 1600, 400)).toBe(1600 - 400 - MIN_WIDTH.middle);
    expect(dragTo('right', 100, 1600, 500)).toBe(1600 - 500 - MIN_WIDTH.middle);
  });
});

describe('saved settings', () => {
  it('reads saved widths and ignores anything else', () => {
    expect(parsePanes('{"left":480,"right":390}')).toEqual({ left: 480, right: 390 });
    expect(parsePanes('{"left":"wide","right":-3}')).toEqual({});
    expect(parsePanes('not json')).toEqual({});
    expect(parsePanes(undefined)).toEqual({});
  });

  it('starts dark unless light was chosen', () => {
    expect(parseTheme(undefined)).toBe('dark');
    expect(parseTheme('light')).toBe('light');
    expect(parseTheme('sepia')).toBe('dark');
  });
});
