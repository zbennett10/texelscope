// Light reading of WGSL source for what the hook can't learn from WebGPU itself.

/** Whether each `@group(g) @binding(b)` buffer is a uniform or a storage buffer, keyed `g:b`. */
export function resourceKinds(code: string): Map<string, 'uniform' | 'storage'> {
  const kinds = new Map<string, 'uniform' | 'storage'>();
  const declaration = /((?:@\s*\w+\s*\([^)]*\)\s*)+)var\s*<\s*(uniform|storage)\b/g;
  for (const match of stripComments(code).matchAll(declaration)) {
    const attributes = match[1]!;
    const group = /@\s*group\s*\(\s*(\d+)\s*\)/.exec(attributes)?.[1];
    const binding = /@\s*binding\s*\(\s*(\d+)\s*\)/.exec(attributes)?.[1];
    if (group !== undefined && binding !== undefined) {
      kinds.set(`${group}:${binding}`, match[2] as 'uniform' | 'storage');
    }
  }
  return kinds;
}

/** The name of the first `@fragment` entry point. */
export function fragmentEntry(code: string): string | undefined {
  return /@\s*fragment\s+fn\s+(\w+)/.exec(stripComments(code))?.[1];
}

function stripComments(code: string): string {
  return code.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}
