// Shapes of the JSON the Rust core returns. crates/texelscope-interp and texelscope-explain
// define them.

export type Value =
  | { bool: boolean }
  | { i32: number }
  | { u32: number }
  | { f32: number | 'NaN' | 'Infinity' | '-Infinity' }
  | { vector: Value[] }
  | { matrix: Value[] }
  | { array: Value[] }
  | { struct: [string, Value][] };

export interface FunctionRef {
  kind: 'function' | 'entryPoint';
  index: number;
}

export interface ExprRef {
  function: FunctionRef;
  expr: number;
}

export interface SourceSpan {
  start: number;
  end: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type Origin =
  | { kind: 'buffer'; group: number; binding: number; path: string; indices: number[] }
  | { kind: 'texels'; group: number; binding: number; name: string; level: number; texels: [number, number, number][] }
  | { kind: 'input'; name: string; location?: number };

export interface Step {
  expr: ExprRef;
  span: SourceSpan | null;
  value: Value;
  inputs: number[];
  guard: number | null;
  depth: number;
  call: number;
  name?: string;
  origin?: Origin;
  note?: string;
}

export interface Branch {
  at: number;
  kind: 'if' | 'select' | 'breakIf' | 'switch';
  condition: ExprRef;
  step: number | null;
  taken: boolean;
  margin: number | null;
  forced: boolean;
  span: SourceSpan | null;
  depth: number;
}

export interface Store {
  at: number;
  target: string;
  value: Value;
  step: number | null;
  span: SourceSpan | null;
  depth: number;
  call: number;
}

export interface Trace {
  entryPoint: string;
  pixel: [number, number];
  steps: Step[];
  branches: Branch[];
  stores: Store[];
  outputs: { location?: number; builtin?: string; value: Value }[];
  result: number | null;
  resultGuard: number | null;
  discarded: boolean;
  truncated: boolean;
  notes: string[];
}

export interface Link {
  kind: 'input' | 'buffer' | 'texture' | 'let' | 'var';
  name: string;
  scope: string | null;
  value: Value;
  detail: string;
  span: SourceSpan | null;
  expr: ExprRef | null;
  step: number | null;
}

export interface Decision {
  condition: string;
  taken: boolean;
  margin: number | null;
  span: SourceSpan | null;
  expr: ExprRef | null;
  count: number;
  forced: boolean;
}

export interface Sensitivity {
  input: string;
  value: number;
  dColor: [number, number, number, number];
}

export interface Explanation {
  pixel: [number, number];
  color: Value;
  discarded: boolean;
  links: Link[];
  decisions: Decision[];
  sensitivities: Sensitivity[];
  notes: string[];
}

export type Override =
  | { kind: 'pin'; expr: ExprRef; value: Value }
  | { kind: 'forceBranch'; condition: ExprRef; taken: boolean }
  | { kind: 'replace'; expr: ExprRef; wgsl: string };

export interface Layer {
  name: string;
  function: string;
  expr: ExprRef | null;
  span: SourceSpan | null;
  components: number;
  min: number[];
  max: number[];
  evaluations: number;
  reached: number;
}

export interface BranchLayer {
  condition: ExprRef;
  span: SourceSpan | null;
  taken: number;
  notTaken: number;
}

export interface RegionData {
  meta: { rect: Rect; layers: Layer[]; branches: BranchLayer[] };
  /** Four floats per pixel for each layer, in the order of `meta.layers`. */
  layers: Float32Array;
  /** One byte per pixel for each branch: 0 not reached, 1 false, 2 true, 3 both. */
  branches: Uint8Array;
  colors: Float32Array;
}

export type ResourceKind =
  | { kind: 'uniform' }
  | { kind: 'storage'; readOnly: boolean }
  | { kind: 'texture'; viewDimension: string; sampleType: string; multisampled: boolean }
  | { kind: 'storageTexture'; viewDimension: string }
  | { kind: 'sampler'; comparison: boolean }
  | { kind: 'other' };

export interface Description {
  entryPoints: { function: FunctionRef; name: string; stage: string }[];
  namedValues: {
    function: FunctionRef;
    functionName: string;
    name: string;
    kind: 'argument' | 'let' | 'var';
    expr: ExprRef | null;
    span: SourceSpan | null;
  }[];
  resources: { group: number; binding: number; name: string; kind: ResourceKind }[];
  stageInputs: { location: number; ty: string; interpolation: string | null; sampling: string | null }[];
}

export const sameExpr = (a: ExprRef | null | undefined, b: ExprRef | null | undefined) =>
  !!a && !!b && a.expr === b.expr && a.function.kind === b.function.kind && a.function.index === b.function.index;
