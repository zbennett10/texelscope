import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  resolve: {
    // The SDK is used from source, so the UI never waits on its build.
    alias: { '@texelscope/sdk': fileURLToPath(new URL('../sdk/src/index.ts', import.meta.url)) },
  },
  server: { port: 5173, strictPort: true },
  worker: { format: 'es' },
});
