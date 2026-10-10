import type { Capture, FragmentInputs } from '../capture.js';

/** Options for {@link installWebGL2}. */
export interface WebGL2InstallOptions {
  /** Picks the programs to watch. Every program is watched by default. */
  filter?: (program: { vertexSource: string; fragmentSource: string }) => boolean;
  /** Fragment inputs to record, for passes whose inputs the hook can't see. */
  inputs?: FragmentInputs;
}

/** A live install of the WebGL2 hook. */
export interface WebGL2Session {
  /** Resolves with a capture of the next draw that uses a watched program. */
  captureNextDraw(): Promise<Capture>;
  /** Puts the original WebGL2 methods back. */
  uninstall(): void;
}

/**
 * Patches WebGL2 in this global scope, a page or a worker, so draws can be captured.
 * Call it before the app compiles its shaders.
 */
export function installWebGL2(_options: WebGL2InstallOptions = {}): WebGL2Session {
  throw new Error('installWebGL2 is not implemented yet');
}
