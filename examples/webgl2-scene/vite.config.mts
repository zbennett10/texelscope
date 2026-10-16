// Serves the scene with the Texelscope hooks, so Alt-clicking a pixel opens Texelscope.
// Run `npm run build` in the repository first, which builds the UI the plugin serves.
import { defineConfig } from 'vite';
import texelscope from '../../packages/vite-plugin/src/index.ts';

export default defineConfig({ plugins: [texelscope()] });
