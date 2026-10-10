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
