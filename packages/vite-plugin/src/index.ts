// A Vite plugin for debugging an app's shaders with no code in the app. In dev it starts the
// Texelscope hooks before the app's own scripts, so Alt-clicking any pixel of a WebGL2 or
// WebGPU canvas captures the draw behind it, and it serves the Texelscope UI from the app's
// dev server at /__texelscope/, where the capture opens.

import { createReadStream, existsSync, statSync } from 'node:fs';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { type Plugin, searchForWorkspaceRoot } from 'vite';

export interface TexelscopeOptions {
  /** The built Texelscope UI. Defaults to `packages/ui/dist` in the Texelscope checkout. */
  uiDir?: string;
  /** How many pixels on a side WebGL captures record inputs for. Defaults to 128. */
  window?: number;
}

/** The module the plugin adds to every page. */
const ENTRY = '/@texelscope/auto';
/** Where the UI is served. */
const BASE = '/__texelscope/';

const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};

export default function texelscope(options: TexelscopeOptions = {}): Plugin {
  // The plugin sits in packages/vite-plugin, next to the SDK and the UI.
  const packages = resolve(fileURLToPath(new URL('.', import.meta.url)), '..', '..');
  const sdk = join(packages, 'sdk', 'src');
  const uiDir = resolve(options.uiDir ?? join(packages, 'ui', 'dist'));
  return {
    name: 'texelscope',
    apply: 'serve',
    config(config) {
      // The hooks come from the SDK's source, outside the app, so Vite has to be allowed to serve it.
      const allow = config.server?.fs?.allow ?? [searchForWorkspaceRoot(config.root ?? process.cwd())];
      return { server: { fs: { allow: [...allow, sdk] } } };
    },
    resolveId(id) {
      return id === ENTRY ? ENTRY : undefined;
    },
    load(id) {
      if (id !== ENTRY) return undefined;
      const auto = JSON.stringify(join(sdk, 'auto.ts'));
      return `import { startAuto } from ${auto};\nstartAuto({ ui: ${JSON.stringify(BASE)}, window: ${options.window ?? 128} });\n`;
    },
    transformIndexHtml: {
      order: 'pre',
      handler: () => [{ tag: 'script', attrs: { type: 'module', src: ENTRY }, injectTo: 'head-prepend' }],
    },
    configureServer(server) {
      server.middlewares.use(BASE, (req, res) => {
        if (!existsSync(join(uiDir, 'index.html'))) {
          res.statusCode = 404;
          res.setHeader('Content-Type', 'text/plain; charset=utf-8');
          res.end(`The Texelscope UI isn't built yet. Run npm run build in the Texelscope checkout, then reload.\nLooked in ${uiDir}`);
          return;
        }
        const path = decodeURIComponent((req.url ?? '/').split('?')[0]!);
        const file = normalize(join(uiDir, path === '/' ? 'index.html' : path));
        if (!file.startsWith(uiDir + sep) || !existsSync(file) || !statSync(file).isFile()) {
          res.statusCode = 404;
          res.end();
          return;
        }
        res.setHeader('Content-Type', TYPES[extname(file)] ?? 'application/octet-stream');
        // A file can go away before it is read, as when the UI is built again, and an unhandled
        // stream error would stop the app's dev server.
        const stream = createReadStream(file);
        stream.on('error', () => {
          if (!res.headersSent) res.statusCode = 404;
          res.end();
        });
        stream.pipe(res);
      });
    },
  };
}
