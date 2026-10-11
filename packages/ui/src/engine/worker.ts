// Runs the Rust core, compiled to WebAssembly, off the main thread.

import init, { Session } from '../../wasm/texelscope_wasm.js';

const ready = init();
let session: Session | undefined;

const need = () => {
  if (!session) throw new Error('no capture is loaded');
  return session;
};

self.onmessage = async (event: MessageEvent<{ id: number; method: string; args: unknown[] }>) => {
  const { id, method, args } = event.data;
  try {
    await ready;
    let result: unknown;
    const transfer: Transferable[] = [];
    switch (method) {
      case 'load':
        session?.free();
        session = undefined;
        session = new Session(args[0] as string);
        result = JSON.parse(session.describe());
        break;
      case 'setSource':
        result = JSON.parse(need().setSource(args[0] as string));
        break;
      case 'trace': {
        const [x, y, overrides] = args as [number, number, unknown];
        result = JSON.parse(need().tracePixel(x, y, JSON.stringify(overrides)));
        break;
      }
      case 'explain': {
        const [x, y, overrides] = args as [number, number, unknown];
        result = JSON.parse(need().explainPixel(x, y, JSON.stringify(overrides)));
        break;
      }
      case 'probe': {
        const [expr, overrides] = args as [unknown, unknown];
        result = JSON.parse(need().probeSource(JSON.stringify(expr), JSON.stringify(overrides)));
        break;
      }
      case 'compare': {
        const [[ax, ay], [bx, by], overrides] = args as [[number, number], [number, number], unknown];
        result = JSON.parse(need().comparePixels(ax, ay, bx, by, JSON.stringify(overrides)));
        break;
      }
      case 'region': {
        const [rect, overrides, scope] = args as [{ x: number; y: number; width: number; height: number }, unknown, unknown];
        const region = need().inspectRegion(rect.x, rect.y, rect.width, rect.height, JSON.stringify(overrides), JSON.stringify(scope ?? null));
        const data = { meta: JSON.parse(region.meta), layers: region.layers, branches: region.branches, colors: region.colors };
        region.free();
        transfer.push(data.layers.buffer, data.branches.buffer, data.colors.buffer);
        result = data;
        break;
      }
      case 'patched':
        result = need().patchedSource(JSON.stringify(args[0]));
        break;
      case 'bufferBytes': {
        const bytes = need().bufferBytes(args[0] as number, args[1] as number);
        transfer.push(bytes.buffer);
        result = bytes;
        break;
      }
      case 'textureBytes': {
        const bytes = need().textureBytes(args[0] as number, args[1] as number);
        transfer.push(bytes.buffer);
        result = bytes;
        break;
      }
      default:
        throw new Error(`unknown method ${method}`);
    }
    self.postMessage({ id, ok: true, result }, { transfer });
  } catch (error) {
    self.postMessage({ id, ok: false, error: error instanceof Error ? error.message : String(error) });
  }
};
