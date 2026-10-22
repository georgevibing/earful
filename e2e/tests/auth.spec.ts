import { expect, test } from '@playwright/test';
import { login } from './helpers';

test('admin API rejects anonymous callers', async ({ request }) => {
  for (const path of ['/api/admin/projects', '/api/admin/feedback', '/api/admin/stats', '/api/admin/tokens']) {
    expect((await request.get(path)).status(), path).toBe(401);
  }
});

test('wrong password shows an error, right password signs in and out', async ({ page }) => {
  await page.goto('/admin/');
  await page.getByLabel('Admin password').fill('definitely-wrong');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('alert')).toBeVisible();

  await login(page);
  await page.getByRole('button', { name: 'Log out' }).click();
  await expect(page.getByLabel('Admin password')).toBeVisible();
  expect((await page.request.get('/api/admin/projects')).status()).toBe(401);
});

test('security headers are set on the admin app', async ({ request }) => {
  const res = await request.get('/admin/');
  expect(res.status()).toBe(200);
  expect(res.headers()['content-security-policy']).toContain("default-src 'self'");
  expect(res.headers()['x-frame-options']).toBe('DENY');
  expect(res.headers()['x-content-type-options']).toBe('nosniff');
});
