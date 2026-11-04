import { expect, test } from '@playwright/test';
import { apiLogin, createProject, login, uniqueSlug } from './helpers';

test('MCP crash evidence is readable and filterable on a phone', async ({ page, request }) => {
  await apiLogin(request);
  const project = await createProject(request, 'Crash workflow', uniqueSlug('crashes'));
  const response = await request.post('/api/admin/tokens', { data: { name: 'crash test', scope: 'write' } });
  const { token } = await response.json();
  const created = await request.post('/mcp', {
    headers: { authorization: 'Bearer ' + token },
    data: { jsonrpc: '2.0', id: 1, method: 'tools/call', params: {
      name: 'create_feedback', arguments: {
        project: project.slug, kind: 'crash', title: 'Inventory SaveError',
        body: 'Game exited during checkpoint save.',
        diagnostics: { stack_trace: 'SaveInventory at inventory.cpp:42', logs: 'x'.repeat(10000), build_id: 'test-build' }
      }
    } }
  });
  expect((await created.json()).result.isError).toBe(false);
  await login(page);
  for (const width of [320, 390, 1280]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/admin/#/feedback?project_id=' + project.id + '&kind=crash');
    await expect(page.getByTestId('feedback-row')).toHaveCount(1);
    await page.getByTestId('feedback-row').click();
    const detail = page.getByRole('complementary', { name: 'Feedback detail' });
    await expect(detail.getByText('SaveInventory at inventory.cpp:42')).toBeVisible();
    await detail.locator('summary').filter({ hasText: /^logs$/ }).click();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await detail.getByRole('button', { name: 'Close detail' }).click();
  }
});
