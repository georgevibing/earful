import { expect, test } from '@playwright/test';
import { BASE_URL, PORT } from '../playwright.config';
import { apiLogin, createProject, uniqueSlug } from './helpers';

// Chrome's Local Network Access check blocks pages that were not themselves
// served from loopback (route.fulfill pages count as such) from reaching
// 127.0.0.1. Real deployments are public to public, so disable it here only.
test.use({ launchOptions: { args: ['--disable-features=LocalNetworkAccessChecks'] } });

test('floating widget works cross origin on a third party site', async ({ page, request }) => {
  await apiLogin(request);
  const slug = uniqueSlug('webapp');
  const project = await createProject(request, 'Third Party App', slug);

  // A fake third party page on another origin, so the widget must use CORS.
  const thirdParty = `http://localhost:${PORT}/third-party-site`;
  await page.route(thirdParty, (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: `<!doctype html><html><body><h1>Some app</h1>
        <script src="${BASE_URL}/widget.js" data-fh-key="${project.ingest_key}" defer></script></body></html>`,
    }),
  );
  await page.goto(thirdParty);
  await page.getByRole('button', { name: /feedback/i }).first().click();
  await page.getByLabel('Description').fill('The checkout button does nothing on Safari.');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByText('Thanks! Your feedback was sent.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeHidden();

  const list = await request.get(`/api/admin/feedback?project_id=${project.id}&status=any`);
  const body = await list.json();
  expect(body.total).toBe(1);
  expect(body.items[0].source).toBe('web');
  expect(body.items[0].meta.url).toBe(thirdParty);
});

test('honeypot submissions are silently dropped', async ({ request }) => {
  await apiLogin(request);
  const project = await createProject(request, 'Honeypot', uniqueSlug('hp'));
  const ch = await (await request.get(`/api/v1/challenge?key=${project.ingest_key}`)).json();
  const res = await request.post('/api/v1/feedback', {
    data: { key: project.ingest_key, kind: 'bug', body: 'buy cheap stuff', challenge: ch.challenge, nonce: '0', website: 'http://spam.example' },
  });
  expect(res.status()).toBe(202);
  const list = await (await request.get(`/api/admin/feedback?project_id=${project.id}&status=any`)).json();
  expect(list.total).toBe(0);
});
