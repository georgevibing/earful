// Regenerates the README screenshots in docs/assets/.
//
//   pnpm build:web && cargo build && node scripts/screenshots.mjs
//
// Starts the debug binary on loopback with a throwaway database, seeds demo data
// (scripts/seed-demo.mjs), captures every screen in light and dark, then stops
// the server. Uses the Playwright install from the e2e package, so no extra deps.

import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { seed } from './seed-demo.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(root, 'e2e', 'package.json'));
const { chromium } = require('@playwright/test');

const binary = process.env.EARFUL_BINARY ?? join(root, 'target', 'debug', 'earful');
if (!existsSync(binary)) throw new Error(`missing ${binary}; run \`pnpm build:web && cargo build\``);

const PASSWORD = 'demo-password-123';
const PORT = Number(process.env.EARFUL_SCREENSHOT_PORT ?? 18_123);
const BASE = `http://127.0.0.1:${PORT}`;
// The browser sees a realistic public host; Playwright proxies it to loopback,
// so snippets in the UI show a real looking URL instead of 127.0.0.1.
const PUBLIC = 'https://feedback.example.com';
const DEMO_SITE_URL = 'https://app.ledgerly.example/';
const OUT = join(root, 'docs', 'assets');
mkdirSync(OUT, { recursive: true });

const dir = mkdtempSync(join(tmpdir(), 'earful-shots-'));
const hash = execFileSync(binary, ['hash-password'], { input: PASSWORD }).toString().trim();
const server = spawn(binary, ['serve'], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: {
    ...process.env,
    RUST_LOG: 'warn',
    EARFUL_BIND: `127.0.0.1:${PORT}`,
    EARFUL_DATABASE_PATH: join(dir, 'demo.db'),
    EARFUL_ADMIN_PASSWORD_HASH: hash,
    EARFUL_COOKIE_SECURE: 'false',
    EARFUL_TRUSTED_PROXY_HOPS: '1',
    EARFUL_POW_BITS: '8',
    EARFUL_POW_MAX_BITS: '12',
    EARFUL_MIN_FILL_SECS: '0',
    EARFUL_EMBED_PROVIDER: 'hash',
  },
});

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/healthz`)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('server did not start');
}

const DEMO_SITE = `<!doctype html><html><head><meta charset="utf-8"><title>Ledgerly</title>
<style>
  body { margin: 0; font: 15px/1.5 system-ui, -apple-system, Segoe UI, sans-serif; background: #f5f7fb; color: #1d2433; }
  header { display: flex; align-items: center; gap: 28px; padding: 18px 40px; background: #fff; border-bottom: 1px solid #e3e8f0; }
  header b { font-size: 19px; color: #2453d4; } header span { color: #5b6578; }
  main { padding: 36px 40px; display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; max-width: 1180px; }
  .card { background: #fff; border: 1px solid #e3e8f0; border-radius: 12px; padding: 22px; }
  .card h3 { margin: 0 0 6px; font-size: 13px; color: #5b6578; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; }
  .card p { margin: 0; font-size: 28px; font-weight: 650; }
  .wide { grid-column: span 2; height: 260px; } .bar { height: 14px; border-radius: 7px; background: #dbe4fb; margin: 14px 0; }
  .bar i { display: block; height: 100%; border-radius: 7px; background: #2453d4; }
  @media (prefers-color-scheme: dark) {
    body { background: #10141c; color: #e6e9ef; } header, .card { background: #171c26; border-color: #273042; }
    .bar { background: #24304a; }
  }
</style></head><body>
<header><b>Ledgerly</b><span>Dashboard</span><span>Budgets</span><span>Bills</span><span>Accounts</span></header>
<main>
  <div class="card"><h3>Balance</h3><p>$12,480</p></div>
  <div class="card"><h3>Spent this month</h3><p>$2,315</p></div>
  <div class="card"><h3>Upcoming bills</h3><p>4</p></div>
  <div class="card wide"><h3>Budgets</h3>
    <div class="bar"><i style="width:72%"></i></div><div class="bar"><i style="width:45%"></i></div>
    <div class="bar"><i style="width:88%"></i></div><div class="bar"><i style="width:30%"></i></div></div>
  <div class="card wide" style="grid-column: span 1"><h3>Savings goal</h3><p>61%</p></div>
</main>
__SCRIPT__
</body></html>`;

const shots = [];
async function shot(page, name) {
  await page.mouse.move(1, 1);
  await page.waitForTimeout(300);
  const file = join(OUT, `${name}.png`);
  await page.screenshot({ path: file });
  shots.push(file);
}

async function capture(browser, scheme, projects) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    colorScheme: scheme,
    timezoneId: 'Europe/Berlin',
  });
  await context.route(`${PUBLIC}/**`, async (route) => {
    const req = route.request();
    const headers = { ...req.headers() };
    // Same origin admin calls would fail the Origin == Host check behind this fake proxy.
    if (new URL(req.url()).pathname.startsWith('/api/admin')) delete headers.origin;
    const response = await route.fetch({ url: req.url().replace(PUBLIC, BASE), headers, maxRedirects: 0 });
    await route.fulfill({ response });
  });
  const page = await context.newPage();
  await page.goto(`${PUBLIC}/admin/`);
  await page.getByLabel('Admin password').fill(PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByRole('button', { name: 'Log out' }).waitFor();

  // Dashboard.
  await page.goto(`${PUBLIC}/admin/#/`);
  await page.locator('svg').first().waitFor();
  await shot(page, `dashboard-${scheme}`);

  // Inbox with a hybrid search and the detail panel open.
  await page.goto(`${PUBLIC}/admin/#/feedback`);
  await page.getByTestId('feedback-row').first().waitFor();
  await page.getByLabel('Search', { exact: true }).fill('game crash');
  await page.waitForTimeout(600);
  await page.getByTestId('feedback-row').filter({ hasText: 'Inventory crash' }).first().click();
  await page.getByRole('complementary', { name: 'Feedback detail' }).getByText('Similar').first().waitFor().catch(() => {});
  await shot(page, `inbox-${scheme}`);

  // Project detail with integration snippets.
  await page.goto(`${PUBLIC}/admin/#/projects/${projects.starfall.id}`);
  await page.getByTestId('ingest-key').waitFor();
  await shot(page, `project-${scheme}`);

  // API tokens and MCP setup.
  await page.goto(`${PUBLIC}/admin/#/tokens`);
  await page.getByRole('heading', { name: 'API tokens' }).waitFor();
  await shot(page, `tokens-${scheme}`);

  // Hosted page, filled in.
  await page.goto(`${PUBLIC}/s/${projects.starfall.slug}`);
  await page.getByLabel('Description').waitFor();
  await page.getByRole('radio', { name: 'Bug report' }).click();
  await page.getByLabel('Title').fill('Ship explodes when docking');
  await page.getByLabel('Description').fill('Docking at the Kepler station with a damaged engine makes the ship explode instead of docking. Happens on every attempt.');
  await page.getByLabel('Contact').fill('nova_pilot#2231');
  await page.getByLabel('Contact').blur();
  await shot(page, `hosted-${scheme}`);

  // Floating widget open on a third party web app.
  const site = DEMO_SITE_URL;
  await page.route(site, (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: DEMO_SITE.replace(
        '__SCRIPT__',
        `<script src="${PUBLIC}/widget.js" data-earful-key="${projects.ledgerly.ingest_key}" data-earful-project="Ledgerly" defer></script>`,
      ),
    }),
  );
  await page.goto(site);
  await page.getByRole('button', { name: 'Feedback', exact: true }).click();
  await page.getByRole('radio', { name: 'Feedback' }).click();
  await page.getByLabel('Description').fill('Would love a yearly view of recurring bills next to the monthly one.');
  await page.getByRole('radio', { name: '4 stars' }).click();
  await page.getByLabel('Description').focus();
  await page.getByLabel('Description').blur();
  await shot(page, `widget-${scheme}`);

  await context.close();
}

try {
  await waitForServer();
  const { projects } = await seed(BASE, PASSWORD);
  const browser = await chromium.launch();
  for (const scheme of ['light', 'dark']) await capture(browser, scheme, projects);
  await browser.close();
  for (const file of shots) console.log(`${(statSync(file).size / 1024).toFixed(0).padStart(5)} KB  ${file.replace(`${root}/`, '')}`);
} finally {
  server.kill('SIGTERM');
  rmSync(dir, { recursive: true, force: true });
}
