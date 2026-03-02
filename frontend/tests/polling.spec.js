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

  // 3. Status should eventually become '已就绪' (Ready) without reload
  // This validates the polling mechanism.
  // Note: Depending on speed, it might jump straight to ready, or show '等待处理' -> '已就绪'.
  // We strictly wait for '已就绪'.
  await expect(row.getByText('已就绪')).toBeVisible({ timeout: 10000 });

  // 4. Verify "查看" link appears in file row when ready
  const previewLink = row.getByRole('link', { name: '查看' });
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

  const previewLink = row.getByRole('link', { name: '查看' });
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

  await expect(row.getByText('已就绪')).toBeVisible();
  await expect(previewLink).toBeVisible();
});
