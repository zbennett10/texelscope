import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  resolve: {
    // The SDK is used from source, so the UI never waits on its build.
    alias: { '@texelscope/sdk': fileURLToPath(new URL('../sdk/src/index.ts', import.meta.url)) },
  },
  // Relative paths let the built UI run from any folder, such as the Vite plugin's or the extension's.
  base: './',
  server: { port: 5173, strictPort: true },
  worker: { format: 'es' },
});
