import { test, expect } from './fixtures'; // Use custom fixtures
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const geojsonPath = path.join(fixturesDir, 'sample.geojson');
const shapefileZip = path.join(fixturesDir, 'roads.zip');

test.beforeEach(async ({ workerServer, request }) => {
  // Reset DB and uploads for this worker before every test
  await workerServer.reset();
  // Initialize and login test user
  await setupTestUser(request);
  await loginUser(request);
});

async function uploadFile(page, filePath) {
  const input = page.getByTestId('file-input');
  await input.setInputFiles(filePath);
}

test('persistence: upload then reload shows file', async ({ page }) => {
  await page.goto('/');
  await uploadFile(page, geojsonPath);

  // Wait specifically for the uploaded status
  const uploadedRow = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: /已就绪|等待处理/ })
    .first();
  await expect(uploadedRow).toBeVisible();

  await page.reload();

  // Wait for table to load
  await expect(page.locator('.table')).toBeVisible();

  const reloadedRow = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: /已就绪|等待处理/ })
    .first();
  await expect(reloadedRow).toBeVisible();
  await expect(reloadedRow.getByText('geojson')).toBeVisible();
});

test('upload geojson and show in list', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();

  await uploadFile(page, geojsonPath);

  const row = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: /已就绪|等待处理/ })
    .first();
  await expect(row).toBeVisible();
  await expect(row.getByText('geojson')).toBeVisible();
});

test('upload shapefile zip and show in list', async ({ page }) => {
  await page.goto('/');
  await uploadFile(page, shapefileZip);

  // Use locator specific to the row AND status
  const row = page.locator('.row', { hasText: 'roads' }).filter({ hasText: /已就绪|等待处理/ });
  await expect(row).toBeVisible();
  await expect(row.getByText('shapefile')).toBeVisible();
});
