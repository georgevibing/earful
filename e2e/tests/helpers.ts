import { expect, type APIRequestContext, type Page } from '@playwright/test';
import { ADMIN_PASSWORD } from '../playwright.config';

export async function login(page: Page) {
  await page.goto('/admin/');
  await page.getByLabel('Admin password').fill(ADMIN_PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('button', { name: 'Log out' })).toBeVisible();
}

/** Logs the API request context in, so tests can seed data quickly. */
export async function apiLogin(request: APIRequestContext) {
  const res = await request.post('/api/admin/login', { data: { password: ADMIN_PASSWORD } });
  expect(res.status()).toBe(204);
}

export function uniqueSlug(prefix: string) {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`;
}

export async function createProject(request: APIRequestContext, title: string, slug: string) {
  const res = await request.post('/api/admin/projects', { data: { title, slug, project_type: 'game' } });
  expect(res.status()).toBe(201);
  return (await res.json()) as { id: number; slug: string; ingest_key: string };
}
