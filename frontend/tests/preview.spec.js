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

  // Wait for upload to complete (could be '已就绪' or '等待处理' depending on timing)
  // We accept either, but ideally we want '已就绪' to ensure processing is done for preview
  await expect(page.locator('.row', { hasText: 'sample' }).getByText(/已就绪|等待处理/)).toBeVisible();

  // 2. Click row to select it (to open sidebar)
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();
  await row.click();
  
  // 3. Find Preview button in Detail Sidebar
  // The sidebar should now be populated
  const sidebar = page.locator('.detail-area');
  await expect(sidebar.getByText('sample')).toBeVisible(); // Check title in sidebar
  
  const previewLink = sidebar.getByRole('link', { name: 'Open Preview' });
  await expect(previewLink).toBeVisible();

  // 4. Click preview link and wait for new page
  const [newPage] = await Promise.all([
    page.context().waitForEvent('page'),
    previewLink.click(),
  ]);

  await newPage.waitForLoadState();
  
  // 5. Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');
  await expect(newPage.getByText('sample')).toBeVisible(); // Filename in header
});
