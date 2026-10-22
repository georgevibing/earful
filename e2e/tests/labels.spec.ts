import { expect, test } from '@playwright/test';
import { apiLogin, createProject, login, uniqueSlug } from './helpers';

test('agents and the mobile UI share label definitions, assignments, filters and theme counts', async ({ page, request }) => {
  await apiLogin(request);
  const project = await createProject(request, 'Theme workflow', uniqueSlug('themes'));
  const tokenResponse = await request.post('/api/admin/tokens', { data: { name: 'theme test', scope: 'write' } });
  const { token } = await tokenResponse.json();
  const rpc = async (name: string, args: object) => {
    const res = await request.post('/mcp', { headers: { authorization: `Bearer ${token}` }, data: { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name, arguments: args } } });
    expect(res.status()).toBe(200);
    const { result } = await res.json();
    expect(result.isError, JSON.stringify(result)).toBe(false);
    return result.structuredContent;
  };
  const ids: number[] = [];
  for (const title of ['Progress lost after loading', 'Controller remapping']) {
    const feedback = await rpc('create_feedback', { project: project.slug, kind: 'bug', title, body: 'A detailed reproducible report about this problem.' });
    ids.push(feedback.id);
  }
  const controls = await rpc('create_label', { project: project.slug, name: 'Controls', color: 'green', description: 'Controller and keyboard input' });
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page);
  await page.goto(`/admin/#/projects/${project.id}`);
  const manager = page.getByRole('region', { name: 'Project labels' });
  await expect(manager.getByText('Controls', { exact: true })).toBeVisible();
  await manager.getByLabel('Label name').fill('Save reliability');
  await manager.getByLabel('Label color').selectOption('blue');
  await manager.getByLabel('Description / when to use').fill('Lost progress or broken save files');
  await manager.getByRole('button', { name: 'Create label' }).click();
  await expect(manager.getByRole('button', { name: 'Edit Save reliability' })).toBeVisible();
  const listed = await rpc('list_labels', { project: project.slug });
  const save = listed.labels.find((l: { name: string }) => l.name === 'Save reliability');
  await rpc('label_feedback', { ids: [ids[1]], label_ids: [controls.id], action: 'add' });

  await page.goto(`/admin/#/feedback?project_id=${project.id}`);
  await expect(page.getByTestId('feedback-row')).toHaveCount(2);
  await page.getByTestId('feedback-row').filter({ hasText: 'Progress lost' }).click();
  const detail = page.getByRole('complementary', { name: 'Feedback detail' });
  await detail.getByRole('checkbox', { name: 'Save reliability' }).check();
  await expect(detail.getByRole('checkbox', { name: 'Save reliability' })).toBeChecked();
  await detail.getByRole('button', { name: 'Close detail' }).click();
  await page.locator('.label-filters summary').click();
  await page.locator('.label-filters').getByRole('checkbox', { name: /Save reliability/ }).check();
  await expect(page.getByTestId('feedback-row')).toHaveCount(1);
  await expect(page.getByTestId('feedback-row')).toContainText('Progress lost');
  const hash = new URL(page.url()).hash;
  expect(hash).toContain('label_ids=');
  await page.reload();
  await expect(page.getByTestId('feedback-row')).toHaveCount(1);

  // Removing the filter's label immediately removes the report from the list.
  await page.getByTestId('feedback-row').click();
  await detail.getByRole('checkbox', { name: 'Save reliability' }).uncheck();
  await expect(page.getByTestId('feedback-row')).toHaveCount(0);
  await detail.getByRole('button', { name: 'Close detail' }).click();
  await page.getByRole('button', { name: 'Clear labels' }).click();
  await expect(page.getByTestId('feedback-row')).toHaveCount(2);
  await page.getByLabel('Select all', { exact: true }).check();
  await page.getByLabel('Bulk label').selectOption(String(save.id));
  await page.getByRole('button', { name: 'Add label', exact: true }).click();
  await expect(page.getByText('2 selected')).toBeHidden();
  const found = await rpc('list_feedback', { project: project.slug, label_ids: [save.id, controls.id], label_match: 'all' });
  expect(found.items.map((f: { id: number }) => f.id)).toEqual([ids[1]]);
  const counts = await rpc('list_labels', { project: project.slug });
  expect(counts.labels.find((l: { id: number }) => l.id === save.id).open_count).toBe(2);

  await page.goto('/admin/#/');
  const overview = page.getByRole('region', { name: 'Theme overview' });
  await expect(overview.getByText('Save reliability', { exact: true })).toBeVisible();
  await expect(overview).toContainText('2 open / 2 total');
  await overview.getByRole('link', { name: 'Save reliability' }).click();
  await expect(page.getByTestId('feedback-row')).toHaveCount(2);

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`/admin/#/projects/${project.id}`);
  await manager.getByRole('button', { name: 'Edit Save reliability' }).click();
  await manager.getByLabel('Label name').fill('Save integrity');
  await manager.getByRole('button', { name: 'Save label' }).click();
  await expect(manager.getByRole('button', { name: 'Delete Save integrity' })).toBeVisible();
  const renamed = await rpc('list_feedback', { project: project.slug, label_ids: [save.id] });
  expect(renamed.items.every((f: { labels: { name: string }[] }) => f.labels.some((l) => l.name === 'Save integrity'))).toBe(true);
  page.once('dialog', (d) => d.accept());
  await manager.getByRole('button', { name: 'Delete Save integrity' }).click();
  await expect(manager.getByRole('button', { name: 'Delete Save integrity' })).toHaveCount(0);
  const remaining = await rpc('list_feedback', { project: project.slug });
  expect(remaining.items).toHaveLength(2);
  expect(remaining.items.find((f: { id: number }) => f.id === ids[1]).labels.map((l: { id: number }) => l.id)).toEqual([controls.id]);
});
