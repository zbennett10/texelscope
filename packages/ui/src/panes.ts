// How wide the source pane on the left and the explanation on the right are. Their inner
// edges can be dragged, and the image in the middle takes what is left.

/** The widths of the side panes, in CSS pixels. */
export interface PaneWidths {
  left: number;
  right: number;
}

export type Side = keyof PaneWidths;

/** The narrowest each pane may get. The middle holds the image. */
export const MIN_WIDTH = { left: 220, middle: 260, right: 280 } as const;

/** A pane's default width, as a share of the window between two bounds. */
const DEFAULT_WIDTH = {
  left: { share: 0.32, least: 300, most: 640 },
  right: { share: 0.26, least: 340, most: 460 },
} as const;

const clamp = (value: number, least: number, most: number) => Math.min(most, Math.max(least, value));

/** The pane widths in a window `width` wide: the ones chosen, or defaults, fitted so the image keeps its minimum. */
export function paneWidths(chosen: Partial<PaneWidths>, width: number): PaneWidths {
  const wanted = (side: Side) => {
    const { share, least, most } = DEFAULT_WIDTH[side];
    return Math.max(MIN_WIDTH[side], chosen[side] ?? clamp(Math.round(width * share), least, most));
  };
  const left = wanted('left');
  const right = wanted('right');
  const over = left + right + MIN_WIDTH.middle - width;
  const extra = left - MIN_WIDTH.left + (right - MIN_WIDTH.right);
  if (over <= 0 || extra <= 0) return { left, right };
  // Both side panes give up width in proportion to what they have above their minimums.
  const keep = Math.max(0, extra - over) / extra;
  return {
    left: MIN_WIDTH.left + Math.floor((left - MIN_WIDTH.left) * keep),
    right: MIN_WIDTH.right + Math.floor((right - MIN_WIDTH.right) * keep),
  };
}

/**
 * The width a side pane gets when its inner edge is dragged to `edge`, measured from the left
 * of a window `width` wide in which the other side pane is `other` wide.
 */
export function dragTo(side: Side, edge: number, width: number, other: number): number {
  const wanted = side === 'left' ? edge : width - edge;
  return Math.round(Math.max(MIN_WIDTH[side], Math.min(width - MIN_WIDTH.middle - other, wanted)));
}

/** Reads widths saved as JSON, keeping only sensible ones. */
export function parsePanes(text: string | undefined): Partial<PaneWidths> {
  let saved: unknown;
  try {
    saved = JSON.parse(text ?? '{}');
  } catch {
    return {};
  }
  const width = (value: unknown) => (typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.round(value) : undefined);
  const record = (saved ?? {}) as Record<string, unknown>;
  const left = width(record.left);
  const right = width(record.right);
  return { ...(left !== undefined && { left }), ...(right !== undefined && { right }) };
}
