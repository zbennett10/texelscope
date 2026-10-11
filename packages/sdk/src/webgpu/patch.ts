const restores: (() => void)[] = [];

/** Replaces a method on a prototype and remembers how to put it back. */
export function patch<T extends object, K extends keyof T>(
    proto: T,
    field: K,
    replace: (original: T[K]) => T[K]
) {
    const original = proto[field];
    proto[field] = replace(original);
    restores.push(() => { proto[field] = original; });
}

/** Restores every patched method, newest first, so stacked patches unwind in order. */
export const restoreAll = () => {
    while (restores.length > 0) restores.pop()!();
};

/** A separate set of patches with its own restore, so one hook can uninstall without the other. */
export function patcher() {
  const own: (() => void)[] = [];
  return {
    patch<T extends object, K extends keyof T>(proto: T, field: K, replace: (original: T[K]) => T[K]) {
      const original = proto[field];
      proto[field] = replace(original);
      own.push(() => {
        proto[field] = original;
      });
    },
    restore() {
      while (own.length > 0) own.pop()!();
    },
  };
}
