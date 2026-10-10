import type { Capture, FragmentInputs } from '../capture.js';
import { installPatches } from './install.js';
import { restoreAll } from './patch.js';
import { readCapture } from './readback.js';
import {
  encoderDevice, frames, passes, pendingByBuffer, pendingByEncoder, pipelines, shaders, type Snapshot,
} from './state.js';

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

let installed = false;

/**
 * Patches WebGPU in this global scope, a page or a worker, so draws can be captured.
 * Call it before the app requests its GPUDevice.
 */
export function installWebGPU(options: WebGPUInstallOptions = {}): WebGPUSession {
  if (typeof GPUDevice === 'undefined') throw new Error('WebGPU is not available in this scope');
  if (installed) throw new Error('installWebGPU is already installed here, so uninstall the first session');
  installed = true;

  const inputs = options.inputs ?? { kind: 'builtins' };
  const waiting: { resolve: (capture: Capture) => void; reject: (error: unknown) => void }[] = [];

  installPatches({
    onDraw(pass) {
      if (waiting.length === 0) return;
      const state = passes.get(pass);
      const descriptor = state?.pipeline ? pipelines.get(state.pipeline) : undefined;
      const fragment = descriptor?.fragment;
      const code = fragment ? shaders.get(fragment.module)?.code : undefined;
      const device = state ? encoderDevice.get(state.encoder) : undefined;
      if (!state || !fragment || code === undefined || !device) return;
      if (options.filter && !options.filter({ label: descriptor?.label, fragmentCode: code })) return;

      const snapshot: Snapshot = {
        device,
        label: descriptor?.label,
        code,
        entryPoint: fragment.entryPoint ?? undefined,
        target: state.target,
        bindGroups: state.bindGroups.flatMap((bound, index) => (bound ? [{ index, ...bound }] : [])),
        frame: frames.count,
      };
      const waiter = waiting.shift()!;
      const pending = pendingByEncoder.get(state.encoder) ?? [];
      pending.push({
        snapshot,
        resolve: (taken) => {
          try {
            readCapture(taken, inputs).then(waiter.resolve, waiter.reject);
          } catch (error) {
            waiter.reject(error);
          }
        },
      });
      pendingByEncoder.set(state.encoder, pending);
    },
    onSubmit(_queue, buffers) {
      for (const buffer of buffers) {
        const pending = pendingByBuffer.get(buffer);
        if (!pending) continue;
        pendingByBuffer.delete(buffer);
        for (const { snapshot, resolve } of pending) resolve(snapshot);
      }
    },
  });

  return {
    captureNextDraw() {
      return new Promise<Capture>((resolve, reject) => waiting.push({ resolve, reject }));
    },
    uninstall() {
      restoreAll();
      installed = false;
      for (const waiter of waiting.splice(0)) waiter.reject(new Error('the WebGPU hook was uninstalled'));
    },
  };
}
