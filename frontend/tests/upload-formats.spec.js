import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  await setupTestUser(request);
  await loginUser(request);
});

// Sample E2E test for new formats (strategy: test one format as representative)
test('upload geojsonseq and show in list', async ({ page }) => {
  const geojsonlPath = path.join(fixturesDir, 'sample.geojsonl');

  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();

  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonlPath);

  const row = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: /已就绪|等待处理/ })
    .first();

  await expect(row).toBeVisible();
  await expect(row.getByText('geojsonl')).toBeVisible();
});

// Note: KML, GPX, and TopoJSON are covered by backend integration tests
// which is sufficient per our test pyramid strategy (fast, reliable, covers API contracts)
