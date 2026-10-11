// Builds the extension into dist: the hook as one script the browser can inject, the files in
// public as they are, and a copy of the built Texelscope UI under ui/.

import { cpSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

const ui = fileURLToPath(new URL('../ui/dist', import.meta.url));

export default defineConfig({
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    minify: false,
    lib: { entry: 'src/hook.ts', formats: ['iife'], name: 'texelscopeHook', fileName: () => 'hook.js' },
  },
  plugins: [
    {
      name: 'texelscope-ui',
      closeBundle() {
        if (!existsSync(ui)) throw new Error(`The Texelscope UI isn't built yet. Run npm run build -w @texelscope/ui first. Looked in ${ui}`);
        cpSync(ui, fileURLToPath(new URL('dist/ui', import.meta.url)), { recursive: true });
      },
    },
  ],
});
