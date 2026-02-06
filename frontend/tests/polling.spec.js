import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

test.beforeEach(async ({ workerServer }) => {
  await workerServer.reset();
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

  // 4. Verify Detail Sidebar also updates if selected
  await row.click();
  const sidebar = page.locator('.detail-area');
  await expect(sidebar.getByText('已就绪')).toBeVisible();
  
  // 5. Preview button should be enabled
  const previewLink = sidebar.getByRole('link', { name: 'Open Preview' });
  await expect(previewLink).not.toHaveClass(/disabled/);
});
