import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Tauri serves the built files itself; the dev server is only for `npm run tauri dev`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: [
        '**/src-tauri/**',
        '**/playwright-report/**',
        '**/test-results/**',
        '**/coverage/**',
      ],
    },
  },
  // Linux runs the page in WebKitGTK, which matches Safari's feature set.
  build: { target: 'safari16', outDir: 'dist' },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    coverage: { include: ['src/**/*.ts'], exclude: ['src/**/*.test.ts', 'src/mocks.ts'] },
  },
});
