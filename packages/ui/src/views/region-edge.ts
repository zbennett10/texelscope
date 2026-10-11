import type { RegionData } from '../types.js';
import type { Card } from './region-cards.js';

/** Where an edge between two colors in a region shows up on the way to the color. */
export interface Edge {
  /** The first card whose values mostly split the pixels the way the colors do. */
  start: Card;
  /** The first card that splits them about as well as any card does. */
  full: Card;
  /** The two colors either side of the edge, as RGB from 0 to 1, or -1 where discarded. */
  colors: [number[], number[]];
}

/** The most pixels a card is scored on. Larger regions are sampled evenly. */
const SAMPLE = 4096;

/**
 * Splits a region's final colors into two groups and finds where, in compute order, the
 * cards start to tell the groups apart and where they do so as well as they ever will.
 * Returns null when the region has one color or no card explains the split.
 */
export function findEdge(region: RegionData, cards: Card[], start = 0.75): Edge | null {
  const split = sample(region);
  if (!split) return null;
  const scores = cards.map((card) => cardScore(region, card, split.pixels, split.side));
  const best = Math.max(0, ...scores);
  if (best < start) return null;
  const first = scores.findIndex((s) => s >= start);
  const full = scores.findIndex((s) => s >= Math.max(start, best * 0.98));
  return { start: cards[first]!, full: cards[Math.max(first, full)]!, colors: split.colors };
}

/** How well each card splits the region's two colors, in the order given, or null with one color. */
export function edgeScores(region: RegionData, cards: Card[]): number[] | null {
  const split = sample(region);
  return split ? cards.map((card) => cardScore(region, card, split.pixels, split.side)) : null;
}

function sample(region: RegionData) {
  const count = region.meta.rect.width * region.meta.rect.height;
  const step = Math.max(1, Math.floor(count / SAMPLE));
  const pixels: number[] = [];
  for (let p = 0; p < count; p += step) pixels.push(p);
  const split = twoColors(region.colors, pixels);
  return split && { pixels, ...split };
}

function cardScore(region: RegionData, card: Card, pixels: number[], side: Uint8Array): number {
  return card.kind === 'branch' ? branchScore(region, card.index, pixels, side) : layerScore(region, card.index, pixels, side);
}

/** Groups pixels into two colors with 2-means, or returns null when they are all close. */
export function twoColors(colors: Float32Array, pixels: number[]): { side: Uint8Array; colors: [number[], number[]] } | null {
  const rgb = (p: number) => [0, 1, 2].map((c) => colors[p * 4 + c] ?? NaN).map((v) => (Number.isNaN(v) ? -1 : v));
  const points = pixels.map(rgb);
  const distance = (a: number[], b: number[]) => Math.hypot(a[0]! - b[0]!, a[1]! - b[1]!, a[2]! - b[2]!);
  // Start from the two colors farthest apart: the one farthest from the first, then the one
  // farthest from that.
  const farthest = (from: number[]) => points.reduce((best, p) => (distance(p, from) > distance(best, from) ? p : best), points[0]!);
  let centers: [number[], number[]] = [farthest(points[0]!), [0, 0, 0]];
  centers[1] = farthest(centers[0]);
  if (distance(centers[0], centers[1]) < 0.05) return null;
  const side = new Uint8Array(points.length);
  for (let round = 0; round < 8; round++) {
    points.forEach((p, i) => (side[i] = distance(p, centers[0]) <= distance(p, centers[1]) ? 0 : 1));
    const sums = [
      [0, 0, 0, 0],
      [0, 0, 0, 0],
    ];
    points.forEach((p, i) => {
      const s = sums[side[i]!]!;
      for (let c = 0; c < 3; c++) s[c]! += p[c]!;
      s[3]!++;
    });
    if (sums[0]![3] === 0 || sums[1]![3] === 0) return null;
    centers = [sums[0]!.slice(0, 3).map((v) => v / sums[0]![3]!), sums[1]!.slice(0, 3).map((v) => v / sums[1]![3]!)];
  }
  if (distance(centers[0], centers[1]) < 0.05) return null;
  return { side, colors: centers };
}

/** How much better than guessing a partition of the pixels predicts their side of the edge. */
function score(groups: Map<number | string, [number, number]>, side: Uint8Array): number {
  let pure = 0;
  for (const [a, b] of groups.values()) pure += Math.max(a, b);
  const ones = side.reduce((n, s) => n + s, 0);
  const baseline = Math.max(ones, side.length - ones);
  return baseline === side.length ? 0 : (pure - baseline) / (side.length - baseline);
}

function tally(groups: Map<number | string, [number, number]>, key: number | string, s: number) {
  const g = groups.get(key) ?? [0, 0];
  g[s]!++;
  groups.set(key, g);
}

/** A decision splits pixels by how it went for each, including not at all. */
function branchScore(region: RegionData, index: number, pixels: number[], side: Uint8Array): number {
  const count = region.meta.rect.width * region.meta.rect.height;
  const data = region.branches.subarray(index * count, (index + 1) * count);
  const groups = new Map<number | string, [number, number]>();
  pixels.forEach((p, i) => tally(groups, data[p]!, side[i]!));
  return score(groups, side);
}

/** A value splits pixels at its best threshold, on whichever component splits them best. */
function layerScore(region: RegionData, index: number, pixels: number[], side: Uint8Array): number {
  const count = region.meta.rect.width * region.meta.rect.height;
  const layer = region.meta.layers[index]!;
  const data = region.layers.subarray(index * count * 4, (index + 1) * count * 4);
  let best = 0;
  for (let c = 0; c < layer.components; c++) {
    const known: { v: number; s: number }[] = [];
    const missing: [number, number] = [0, 0];
    pixels.forEach((p, i) => {
      const v = data[p * 4 + c]!;
      if (Number.isNaN(v)) missing[side[i]!]!++;
      else known.push({ v, s: side[i]! });
    });
    known.sort((a, b) => a.v - b.v);
    const total: [number, number] = [0, 0];
    for (const k of known) total[k.s]!++;
    // Pixels that never computed it form their own group on either side of any threshold.
    const groups = new Map<number | string, [number, number]>([
      ['missing', missing],
      ['below', [0, 0]],
      ['above', [total[0], total[1]]],
    ]);
    best = Math.max(best, score(groups, side));
    const below = groups.get('below')!;
    const above = groups.get('above')!;
    for (let k = 0; k < known.length; k++) {
      below[known[k]!.s]!++;
      above[known[k]!.s]!--;
      if (k + 1 < known.length && known[k + 1]!.v === known[k]!.v) continue;
      best = Math.max(best, score(groups, side));
    }
  }
  return best;
}
