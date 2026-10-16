// What the inspector reads for a picked pixel: the CPU's explanation and trace, and the GPU's color.

/**
 * Asks the CPU for a pixel's explanation and trace, then reads its GPU color. The color comes
 * last, so a Live frame that shows while the CPU works isn't replaced by an older one.
 */
export async function inspectPixel<E, T, G>(explain: () => Promise<E>, trace: () => Promise<T>, gpu: () => Promise<G>): Promise<[E, T, G]> {
  const [explanation, steps] = await Promise.all([explain(), trace()]);
  return [explanation, steps, await gpu()];
}
