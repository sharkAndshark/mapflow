import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  // Initialize and login test user
  await setupTestUser(request);
  await loginUser(request);
});

test('upload file and verify status auto-updates from processing to ready', async ({ page }) => {
  // 1. Upload a file
  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  // 2. Wait for it to appear in list (optimistic or uploaded)
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();

  await expect(row.getByTestId(/status-ready/)).toBeVisible({ timeout: 10000 });

  const previewLink = row.getByTestId('preview-link');
  await expect(previewLink).toBeVisible();
});

test('preview action is hidden before ready and shown after ready', async ({ page, request }) => {
  test.setTimeout(120000);

  const fixturesDir = path.join(__dirname, 'fixtures');
  const shapefileZip = path.join(fixturesDir, 'roads.zip');

  await page.goto('/');
  const input = page.getByTestId('file-input');
  await input.setInputFiles(shapefileZip);

  const row = page.locator('.row', { hasText: 'roads' });
  await expect(row).toBeVisible();

  const previewLink = row.getByTestId('preview-link');
  await expect(previewLink).toHaveCount(0);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const roads = files.find((f) => f.name === 'roads');
        return roads?.status;
      },
      {
        message: 'wait for roads file to be ready',
        timeout: 60000,
      },
    )
    .toBe('ready');

  await expect(row.getByTestId(/status-ready/)).toBeVisible();
  await expect(previewLink).toBeVisible();
});
