import type { Capture, FragmentInputs } from '../capture.js';

/** Options for {@link installWebGPU}. */
export interface WebGPUInstallOptions {
  /** Picks the pipelines to watch. Every pipeline is watched by default. */
  filter?: (pipeline: { label?: string; fragmentCode: string }) => boolean;
  /** Fragment inputs to record, for passes whose inputs the hook can't see. */
  inputs?: FragmentInputs;
}

/** A live install of the WebGPU hook. */
export interface WebGPUSession {
  /** Resolves with a capture of the next draw that uses a watched pipeline. */
  captureNextDraw(): Promise<Capture>;
  /** Puts the original WebGPU methods back. */
  uninstall(): void;
}

/**
 * Patches WebGPU in this global scope, a page or a worker, so draws can be captured.
 * Call it before the app requests its GPUDevice.
 */
export function installWebGPU(_options: WebGPUInstallOptions = {}): WebGPUSession {
  throw new Error('installWebGPU is not implemented yet');
}
