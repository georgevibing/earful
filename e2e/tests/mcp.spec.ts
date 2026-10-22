import { expect, test } from '@playwright/test';
import { login, uniqueSlug } from './helpers';

test('create an API token in the UI and drive the MCP endpoint with it', async ({ page, request }) => {
  await login(page);
  await page.getByRole('link', { name: 'API tokens' }).click();
  await page.getByLabel('Name').fill('e2e agent');
  await page.getByLabel('Scope').selectOption('write');
  await page.getByRole('button', { name: 'Create token' }).click();
  const token = (await page.getByTestId('new-token').locator('code').first().textContent())?.trim() ?? '';
  expect(token).toMatch(/^fht_[0-9a-f]{64}$/);

  const rpc = async (method: string, params: unknown = {}, id = 1) => {
    const res = await request.post('/mcp', {
      headers: { authorization: `Bearer ${token}`, accept: 'application/json, text/event-stream' },
      data: { jsonrpc: '2.0', id, method, params },
    });
    expect(res.status()).toBe(200);
    return (await res.json()) as { result?: any; error?: any };
  };

  const init = await rpc('initialize', {
    protocolVersion: '2025-06-18',
    capabilities: {},
    clientInfo: { name: 'e2e', version: '1' },
  });
  expect(init.result.serverInfo.name).toBe('feedback-harvest');

  const tools = await rpc('tools/list');
  const names = tools.result.tools.map((t: { name: string }) => t.name);
  expect(names).toEqual(expect.arrayContaining(['create_project', 'search_feedback', 'update_feedback']));

  const slug = uniqueSlug('mcp');
  const created = await rpc('tools/call', { name: 'create_project', arguments: { title: 'MCP Project', slug } });
  expect(created.result.isError ?? false).toBe(false);

  const fb = await rpc('tools/call', {
    name: 'create_feedback',
    arguments: { project: slug, kind: 'bug', title: 'Imported from Discord', body: 'Audio cuts out after alt-tab.' },
  });
  expect(fb.result.isError ?? false).toBe(false);

  const found = await rpc('tools/call', { name: 'search_feedback', arguments: { q: 'audio alt tab', project: slug } });
  expect(JSON.stringify(found.result.structuredContent)).toContain('Imported from Discord');

  // The project created by the agent shows up in the admin UI.
  await page.getByRole('link', { name: 'Projects', exact: true }).click();
  await expect(page.getByRole('link', { name: 'MCP Project' }).first()).toBeVisible();

  // Revoking the token locks the agent out.
  await page.getByRole('link', { name: 'API tokens' }).click();
  page.once('dialog', (d) => d.accept());
  await page.getByRole('button', { name: 'Revoke e2e agent' }).click();
  await expect(page.getByRole('button', { name: 'Revoke e2e agent' })).toHaveCount(0);
  const denied = await request.post('/mcp', {
    headers: { authorization: `Bearer ${token}` },
    data: { jsonrpc: '2.0', id: 9, method: 'tools/list' },
  });
  expect(denied.status()).toBe(401);
});
