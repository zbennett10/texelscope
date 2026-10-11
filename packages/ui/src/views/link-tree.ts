/** Where a value in the list from data to color ran: its call depth, call and function. */
export interface LinkPlace {
  /** The value's index in the explanation's links. */
  index: number;
  depth: number;
  /** Which call of its function computed it, so two calls in a row stay apart. */
  call: number;
  scope: string | null;
}

/** A value, or a function call holding the values computed inside it. */
export type LinkNode =
  | { kind: 'row'; index: number }
  | { kind: 'call'; key: string; name: string; size: number; children: LinkNode[] };

/**
 * Nests values under the function calls that computed them, so a list can fold each call
 * away. Keys name a call by its path and how many calls with that path came before it, so
 * they survive a new explanation of the same shader. A call with one value stays a row.
 */
export function callTree(places: LinkPlace[]): LinkNode[] {
  const seen = new Map<string, number>();
  const build = (rows: LinkPlace[], depth: number, path: string): LinkNode[] => {
    const nodes: LinkNode[] = [];
    let i = 0;
    while (i < rows.length) {
      const row = rows[i]!;
      if (row.depth <= depth) {
        nodes.push({ kind: 'row', index: row.index });
        i++;
        continue;
      }
      // Deeper rows belong to one call until a row at that call's depth comes from another.
      let end = i;
      let call: number | undefined;
      while (end < rows.length && rows[end]!.depth > depth) {
        const r = rows[end]!;
        if (r.depth === depth + 1) {
          if (call === undefined) call = r.call;
          else if (r.call !== call) break;
        }
        end++;
      }
      const run = rows.slice(i, end);
      const name = (run.find((r) => r.depth === depth + 1) ?? run[0]!).scope ?? 'call';
      const base = `${path}/${name}`;
      const count = seen.get(base) ?? 0;
      seen.set(base, count + 1);
      const key = `${base}#${count}`;
      const children = build(run, depth + 1, key);
      if (run.length === 1) nodes.push(...children);
      else nodes.push({ kind: 'call', key, name, size: run.length, children });
      i = end;
    }
    return nodes;
  };
  return build(places, 0, '');
}
