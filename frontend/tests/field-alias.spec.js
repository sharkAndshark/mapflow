import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const geojsonPath = path.join(fixturesDir, 'sample.geojson');

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  await setupTestUser(request);
  await loginUser(request);
});

async function uploadAndWaitReady(page) {
  await page.goto('/');
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);
  const row = page
    .locator('.row', { hasText: 'sample' })
    .filter({ has: page.getByTestId('status-ready') })
    .first();
  await expect(row).toBeVisible({ timeout: 15000 });
  return row;
}

async function openFieldsTab(page, row) {
  // Click the row to select it
  await row.click();
  // Switch to Fields tab
  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();
  await sidebar.getByTestId('detail-tab-fields').click();
  // Wait for fields table to load
  await expect(page.locator('.fields-table')).toBeVisible();
}

test('click alias cell enters edit mode', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  // Click alias cell (second column in first data row)
  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  // Verify input appears and is focused
  const input = aliasCell.getByRole('textbox');
  await expect(input).toBeVisible();
  await expect(input).toBeFocused();
});

test('Enter saves alias', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  const input = aliasCell.getByRole('textbox');
  await input.fill('测试别名');
  await input.press('Enter');

  // Wait for edit mode to close
  await expect(aliasCell.getByRole('textbox')).not.toBeVisible();

  // Verify saved value is displayed
  await expect(aliasCell).toContainText('测试别名');
});

test('Esc cancels edit', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');

  // Get original value
  const originalText = await aliasCell.textContent();

  await aliasCell.click();
  const input = aliasCell.getByRole('textbox');
  await input.fill('不应该保存');
  await input.press('Escape');

  // Verify edit mode closed
  await expect(aliasCell.getByRole('textbox')).not.toBeVisible();

  // Verify original value restored
  await expect(aliasCell).toContainText(originalText.trim() || '-');
});

test('click outside cancels edit', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  const input = aliasCell.getByRole('textbox');
  await input.fill('不应该保存');

  // Click outside (on the header)
  await page.locator('.fields-table th').first().click();

  // Verify edit mode closed and value not saved
  await expect(aliasCell.getByRole('textbox')).not.toBeVisible();
  await expect(aliasCell).not.toContainText('不应该保存');
});

test('save and cancel buttons visible in edit mode', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  // Verify buttons are visible
  await expect(aliasCell.getByTestId('alias-save-button')).toBeVisible();
  await expect(aliasCell.getByTestId('alias-cancel-button')).toBeVisible();
});

test('alias input has sufficient width', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  const input = aliasCell.getByRole('textbox');
  const box = await input.boundingBox();

  // Verify input width >= 80px (min-width from CSS)
  expect(box.width).toBeGreaterThanOrEqual(80);
});

test('hover shows clickable hint', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');

  // Verify cursor is pointer
  await expect(aliasCell).toHaveCSS('cursor', 'pointer');
});

test('empty alias shows placeholder', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');

  // Verify "-" is shown when no alias is set
  const text = await aliasCell.textContent();
  expect(text.trim()).toBe('-');
});

test('alias length validation shows error', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  const input = aliasCell.getByRole('textbox');

  // Create a string longer than 255 characters
  const longAlias = 'a'.repeat(256);
  await input.fill(longAlias);
  await input.press('Enter');

  // Verify error message appears
  await expect(aliasCell.getByTestId('alias-error')).toBeVisible();

  // Verify edit mode is still active (not saved)
  await expect(aliasCell.getByRole('textbox')).toBeVisible();
});

test('alias persists after reload', async ({ page }) => {
  const row = await uploadAndWaitReady(page);
  await openFieldsTab(page, row);

  // Set an alias
  const aliasCell = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await aliasCell.click();

  const input = aliasCell.getByRole('textbox');
  await input.fill('持久化测试');
  await input.press('Enter');

  // Wait for save
  await expect(aliasCell.getByRole('textbox')).not.toBeVisible();
  await expect(aliasCell).toContainText('持久化测试');

  // Reload page
  await page.reload();

  // Select row again
  const rowAfterReload = page
    .locator('.row', { hasText: 'sample' })
    .filter({ has: page.getByTestId(/status-ready|status-uploaded|status-processing/) })
    .first();
  await rowAfterReload.click();

  // Switch to Fields tab
  await page.locator('.detail-sidebar').getByTestId('detail-tab-fields').click();

  // Verify alias persisted
  const aliasCellAfterReload = page.locator('.fields-table tbody tr:first-child td.alias-cell');
  await expect(aliasCellAfterReload).toContainText('持久化测试');
});
