import { test, expect } from '@playwright/test';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const uploadDir = path.resolve(__dirname, '../../tmp/test-uploads');
const dbPath = path.resolve(__dirname, '../../tmp/test-mapflow.duckdb');

test.beforeEach(() => {
  fs.rmSync(uploadDir, { recursive: true, force: true });
  fs.rmSync(dbPath, { force: true });
  fs.mkdirSync(uploadDir, { recursive: true });
  // Note: We don't write index.json anymore because backend uses DuckDB.
  // Instead, we rely on the test running against a fresh DB (cleared above).
});

test('click preview opens new tab with map', async ({ page }) => {
  // 1. Upload a file via UI (since we can't easily seed DuckDB from here without a tool)
  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');
  await expect(page.getByTestId('empty-state')).toBeVisible();

  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  // Wait for upload to complete
  await expect(page.getByText('已上传')).toBeVisible();

  // 2. Click preview link and wait for new page
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();
  
  const previewLink = row.locator('a', { hasText: 'Preview' });
  await expect(previewLink).toBeVisible();

  // Validate new tab behavior
  const [newPage] = await Promise.all([
    page.context().waitForEvent('page'),
    previewLink.click(),
  ]);

  await newPage.waitForLoadState();
  
  // 3. Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');
  await expect(newPage.getByText('sample')).toBeVisible(); // Filename in header
});
