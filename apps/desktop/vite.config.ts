import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath, URL } from 'node:url';

export default defineConfig({
  plugins: [tailwindcss(), svelte(), svelteTesting()],
  resolve: { alias: { $lib: fileURLToPath(new URL('./src/lib', import.meta.url)) } },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: ['es2022', 'safari16.4'], sourcemap: false, cssCodeSplit: true, manifest: true },
  test: { environment: 'jsdom', setupFiles: ['./tests/setup.ts'], include: ['src/**/*.test.ts'] },
});
