import { resolve } from 'node:path';
import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.EARFUL_MOBILE_BASE_URL ?? 'http://127.0.0.1:4173';

// Exercise responsive UI with deterministic API fixtures, without building Rust.
export default defineConfig({
  testDir: './tests',
  testMatch: 'mobile.spec.ts',
  workers: 1,
  use: { baseURL, trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: process.env.EARFUL_MOBILE_BASE_URL ? undefined : {
    command: 'exec node node_modules/vite/bin/vite.js --port 4173 --strictPort',
    cwd: resolve(import.meta.dirname, '../apps/admin'),
    url: 'http://127.0.0.1:4173/admin/',
    reuseExistingServer: false,
    gracefulShutdown: { signal: 'SIGTERM', timeout: 1000 },
  },
});
