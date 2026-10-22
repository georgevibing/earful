import { expect, test } from '@playwright/test';
import { login, uniqueSlug } from './helpers';

test('create a project, submit via hosted page, find it with fuzzy search, triage it', async ({ page }) => {
  const slug = uniqueSlug('quest');
  const title = `Quest Game ${slug}`;

  await login(page);

  // Create the project through the UI.
  await page.getByRole('link', { name: 'Projects', exact: true }).click();
  await page.getByRole('link', { name: 'New project' }).click();
  await page.getByLabel('Title').fill(title);
  await page.getByLabel(/^Slug/).fill(slug);
  await page.getByLabel('Type').selectOption('game');
  await page.getByRole('button', { name: 'Create project' }).click();
  await expect(page.getByTestId('ingest-key')).toContainText('fhk_');

  // A player submits a bug report through the hosted page (real PoW in the browser).
  const player = await page.context().newPage();
  await player.goto(`/s/${slug}`);
  await expect(player.getByRole('heading', { name: `Feedback for ${title}` })).toBeVisible();
  await player.getByRole('radio', { name: 'Bug report' }).click();
  await player.getByLabel('Title').fill('Inventory crash on boss fight');
  await player.getByLabel('Description').fill('Opening the inventory during the dragon boss fight crashes the game to desktop.');
  await player.getByLabel('Contact').fill('player@example.com');
  await player.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(player.getByText('Thanks! Your feedback was sent.')).toBeVisible();

  // And a feature request.
  await player.getByRole('button', { name: 'Send another' }).click();
  await player.getByRole('radio', { name: 'Feedback' }).click();
  await player.getByLabel('Title').fill('Please add controller remapping');
  await player.getByLabel('Description').fill('Would love to rebind the gamepad buttons, the default layout is awkward.');
  await player.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(player.getByText('Thanks! Your feedback was sent.')).toBeVisible();
  await player.close();

  // Both land in the inbox.
  await page.getByRole('link', { name: 'Feedback', exact: true }).click();
  await page.getByLabel('Project').selectOption({ label: title });
  await expect(page.getByTestId('feedback-row')).toHaveCount(2);

  // Fuzzy search tolerates typos.
  await page.getByLabel('Search mode').selectOption('fuzzy');
  await page.getByLabel('Search', { exact: true }).fill('invetnory crsh');
  await expect(page.getByTestId('feedback-row')).toHaveCount(1);
  await expect(page.getByTestId('feedback-row').first()).toContainText('Inventory crash');

  // Keyword search finds the other one.
  await page.getByLabel('Search mode').selectOption('keyword');
  await page.getByLabel('Search', { exact: true }).fill('remapping');
  await expect(page.getByTestId('feedback-row')).toHaveCount(1);
  await expect(page.getByTestId('feedback-row').first()).toContainText('controller remapping');

  // Open the bug and triage it.
  await page.getByLabel('Search', { exact: true }).fill('');
  await page.getByTestId('feedback-row').filter({ hasText: 'Inventory crash' }).click();
  const detail = page.getByRole('complementary', { name: 'Feedback detail' });
  await expect(detail).toContainText('player@example.com');
  await detail.getByRole('group', { name: 'Status' }).getByRole('button', { name: 'triaged' }).click();
  await expect(detail.getByRole('button', { name: 'triaged' })).toHaveAttribute('aria-pressed', 'true');

  await page.getByRole('tab', { name: 'Triaged' }).click();
  await expect(page.getByTestId('feedback-row')).toHaveCount(1);
  await expect(page.getByTestId('feedback-row').first()).toContainText('Inventory crash');
});
