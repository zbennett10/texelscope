import type { BranchLayer, Layer, RegionData } from '../types.js';

/** One card of the region strip, by its index among the region's layers or branches. */
export interface Card {
  kind: 'layer' | 'branch';
  index: number;
}

/** A region's cards, split by whether they tell pixels apart. */
export interface Arranged {
  /** Values and decisions that differ between pixels, or that some never reach, in compute order. */
  varying: Card[];
  /** Values and decisions with one answer for every pixel, in compute order. */
  same: Card[];
  /** The part of `same` that comes only from settings, listed after the rest. */
  settings: Card[];
}

/** Sorts a region's values and decisions into one compute order and splits off what doesn't vary. */
export function arrange(meta: RegionData['meta']): Arranged {
  const pixels = meta.rect.width * meta.rect.height;
  const cards = [
    ...meta.layers.map((layer, index) => ({
      card: { kind: 'layer', index } as Card,
      order: layer.order,
      setting: layer.setting,
      same: layerIsSame(layer, pixels),
    })),
    ...meta.branches.map((branch, index) => ({
      card: { kind: 'branch', index } as Card,
      order: branch.order,
      setting: branch.setting,
      same: branchIsSame(branch, pixels),
    })),
  ].sort((a, b) => a.order - b.order);
  return {
    varying: cards.filter((c) => !c.same).map((c) => c.card),
    same: cards.filter((c) => c.same && !c.setting).map((c) => c.card),
    settings: cards.filter((c) => c.same && c.setting).map((c) => c.card),
  };
}

/** Whether every pixel computed a layer and got the same value. */
export function layerIsSame(layer: Layer, pixels: number): boolean {
  if (layer.reached < pixels) return false;
  for (let c = 0; c < layer.components; c++) if (layer.min[c] !== layer.max[c]) return false;
  return true;
}

/** Whether every pixel reached a decision and went the same single way. */
export function branchIsSame(branch: BranchLayer, pixels: number): boolean {
  return branch.taken === pixels || branch.notTaken === pixels;
}
