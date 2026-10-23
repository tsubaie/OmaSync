import { defineConfig, devices } from '@playwright/test';

// The page runs in Vite with the Rust side mocked (src/mocks.ts). WebKit is the closest browser
// to WebKitGTK, which Tauri uses on Linux.
export default defineConfig({
  testDir: './e2e',
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: 'http://localhost:1421',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    reducedMotion: 'reduce',
  },
  webServer: {
    command: 'pnpm exec vite --mode e2e --port 1421 --strictPort',
    url: 'http://localhost:1421',
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    {
      name: process.env.E2E_BROWSER ?? 'webkit',
      use: {
        ...devices[process.env.E2E_BROWSER === 'chromium' ? 'Desktop Chrome' : 'Desktop Safari'],
      },
    },
  ],
});
