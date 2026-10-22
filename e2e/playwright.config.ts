import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { defineConfig, devices } from '@playwright/test';

// Runs the real server binary (debug build) against a throwaway database.
const root = resolve(import.meta.dirname, '..');
const binary = process.env.FH_E2E_BINARY ?? join(root, 'target', 'debug', 'feedback-harvest');
if (!existsSync(binary)) {
  throw new Error(`server binary not found at ${binary}; run \`pnpm build:web && cargo build\` first`);
}

export const ADMIN_PASSWORD = 'e2e-password-123';
export const PORT = Number(process.env.FH_E2E_PORT ?? 18_080);
export const BASE_URL = `http://127.0.0.1:${PORT}`;

// Config is evaluated in every worker; env vars set here are inherited, so the
// temp dir and hash are created once by the main process.
process.env.FH_E2E_DIR ??= mkdtempSync(join(tmpdir(), 'fh-e2e-'));
process.env.FH_E2E_HASH ??= execFileSync(binary, ['hash-password'], { input: ADMIN_PASSWORD })
  .toString()
  .trim();

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: BASE_URL,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: `${binary} serve`,
    url: `${BASE_URL}/healthz`,
    reuseExistingServer: false,
    timeout: 30_000,
    stdout: 'pipe',
    env: {
      RUST_LOG: 'warn',
      FH_BIND: `127.0.0.1:${PORT}`,
      FH_DATABASE_PATH: join(process.env.FH_E2E_DIR, 'e2e.db'),
      FH_ADMIN_PASSWORD_HASH: process.env.FH_E2E_HASH,
      FH_COOKIE_SECURE: 'false',
      FH_POW_BITS: '8',
      FH_POW_MAX_BITS: '10',
      FH_MIN_FILL_SECS: '0',
      // Every test shares 127.0.0.1, so relax the per IP limits.
      FH_LIMIT_IP_PROJECT_MINUTE: '100',
      FH_LIMIT_IP_PROJECT_HOUR: '1000',
      FH_LIMIT_IP_PROJECT_DAY: '1000',
      FH_LIMIT_IP_GLOBAL_DAY: '1000',
      FH_LIMIT_CHALLENGES_IP_HOUR: '1000',
      FH_EMBED_PROVIDER: 'hash',
    },
  },
});
