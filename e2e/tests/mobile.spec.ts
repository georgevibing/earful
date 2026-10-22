import { expect, test, type Page } from '@playwright/test';
import type { Feedback, Project, Stats } from '../../apps/admin/src/types';

const longName = 'Space exploration project ' + 'x'.repeat(90);
const project: Project = {
  id: 1, title: longName, slug: 'space-' + 'x'.repeat(55), project_type: 'game',
  description: 'A long description with an unbroken URL: https://example.com/' + 'x'.repeat(150),
  ingest_key: 'fhk_' + 'a'.repeat(32), accepting: true, created_at: 1_700_000_000, updated_at: 1_700_000_000,
  stats: { total: 1, open: 1, new: 1, bugs: 1, feedback: 0, spam: 0, last_at: 1_700_000_000 },
};
const feedback: Feedback = {
  id: 1, project_id: 1, kind: 'bug', title: 'A very long crash report ' + 'x'.repeat(150),
  body: 'Crash details: ' + 'x'.repeat(250), contact: 'x'.repeat(120) + '@example.com',
  rating: 4, meta: { url: 'https://example.com/' + 'x'.repeat(200) }, source: 'web', status: 'new',
  spam_reason: null, spam_score: 0, admin_note: '', dup_count: 0,
  created_at: 1_700_000_000, updated_at: 1_700_000_000, last_seen_at: 1_700_000_000,
};
const stats: Stats = {
  totals: { total: 1, open: 1, new: 1, bugs: 1, feedback: 0, spam: 0, last_7d: 1 },
  daily: [{ day: '2026-10-21', bug: 1, feedback: 0 }, { day: '2026-10-22', bug: 0, feedback: 0 }],
  blocked: { rate_limited: 2, spam_score: 1 },
  embeddings: { enabled: true, model: 'model2vec:potion-base-8M', embedded: 1, pending: 0 },
};

async function mockApi(page: Page) {
  await page.route('**/api/admin/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/logout')) return route.fulfill({ status: 204 });
    const data = path.endsWith('/me') ? { ok: true }
      : path.endsWith('/stats') ? stats
      : path.endsWith('/projects') ? [project]
      : path.endsWith('/projects/1') ? project
      : path.endsWith('/feedback/1') ? { item: feedback, similar: [] }
      : path.endsWith('/feedback') ? { items: [feedback], total: 1, mode: 'hybrid', semantic: true }
      : path.endsWith('/tokens') ? (route.request().method() === 'POST'
        ? { id: 1, name: longName, scope: 'read', prefix: 'fht_ab12', token: 'fht_' + 'a'.repeat(64), created_at: 1_700_000_000, last_used_at: null }
        : [{ id: 1, name: longName, scope: 'read', prefix: 'fht_ab12', created_at: 1_700_000_000, last_used_at: null }])
      : null;
    if (data === null) throw new Error(`Unexpected API request: ${path}`);
    await route.fulfill({ json: data });
  });
}

async function fitsViewport(page: Page) {
  await expect.poll(() => page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    document: document.documentElement.scrollWidth,
  }))).toEqual({ viewport: page.viewportSize()!.width, document: page.viewportSize()!.width });
}

for (const width of [320, 390, 768, 1280]) {
  test.describe(`${width}px layout`, () => {
    test.use({ viewport: { width, height: 844 } });
    test.beforeEach(async ({ page }) => { await mockApi(page); });

    test('all navigation links stay visible and work', async ({ page }) => {
      await page.goto('/admin/');
      const nav = page.getByRole('navigation');
      for (const label of ['Dashboard', 'Feedback', 'Projects', 'API tokens']) {
        const link = nav.getByRole('link', { name: label, exact: true });
        await expect(link).toBeInViewport({ ratio: 1 });
        await link.click();
        await expect(link).toHaveAttribute('aria-current', 'page');
        await fitsViewport(page);
      }
      await expect(page.getByRole('button', { name: 'Log out' })).toBeInViewport({ ratio: 1 });
      const brand = await page.locator('.brand').boundingBox();
      const navigation = await nav.boundingBox();
      expect(brand && navigation).toBeTruthy();
      expect(brand!.y + brand!.height <= navigation!.y || brand!.x + brand!.width <= navigation!.x).toBeTruthy();
      await page.getByRole('button', { name: 'Log out' }).click();
      await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
      await fitsViewport(page);
    });

    for (const [path, ready] of [
      ['/', '.kpis'], ['/projects', '.table-wrap'], ['/projects/1', '.key-box'],
      ['/projects/1/edit', '.form-grid'], ['/feedback?open=1', '.detail'], ['/tokens', '.table-wrap'],
    ]) {
      test(`${path} contains long content without page overflow`, async ({ page }) => {
        await page.goto('/admin/#' + path);
        await expect(page.locator(ready).first()).toBeVisible();
        await fitsViewport(page);
        if (path === '/projects') {
          const table = page.locator('.table-wrap table');
          // A page can fit the viewport while its columns become unreadable.
          for (const heading of ['Type', 'Open', 'Total', 'Spam', 'Last submission']) {
            const header = table.getByRole('columnheader', { name: heading, exact: true });
            const height = await header.evaluate((el) => el.getBoundingClientRect().height);
            expect(height).toBeLessThan(50);
          }
          const type = table.getByRole('cell', { name: 'game', exact: true });
          expect(await type.evaluate((el) => el.getBoundingClientRect().width)).toBeGreaterThan(40);
          if (width < 700) {
            const scroller = page.locator('.table-wrap');
            expect(await scroller.evaluate((el) => el.scrollWidth > el.clientWidth)).toBeTruthy();
            await scroller.evaluate((el) => { el.scrollLeft = el.scrollWidth; });
            await expect(table.getByRole('columnheader', { name: 'Last submission' })).toBeInViewport({ ratio: 1 });
            await fitsViewport(page);
          }
        }
        if (path === '/feedback?open=1') {
          await page.getByLabel('Date range').selectOption('custom');
          await expect(page.getByLabel('From', { exact: true })).toBeVisible();
          await fitsViewport(page);
          await page.getByRole('tab', { name: 'All', exact: true }).click();
          await expect(page.getByRole('tab', { name: 'All', exact: true })).toHaveAttribute('aria-selected', 'true');
          await fitsViewport(page);
        }
        if (path === '/tokens') {
          await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Mobile client');
          await page.getByRole('button', { name: 'Create token', exact: true }).click();
          await expect(page.getByTestId('new-token')).toBeVisible();
          await fitsViewport(page);
        }
      });
    }
  });
}
