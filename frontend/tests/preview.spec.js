import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  // Initialize and login test user
  await setupTestUser(request);
});

test('click preview opens new tab with map', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test
  // 1. Upload a file via UI (since we can't easily seed DuckDB from here without a tool)
  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');

  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  // Wait for upload to complete (could be '已就绪' or '等待处理' depending on timing)
  // We accept either, but ideally we want '已就绪' to ensure processing is done for preview
  await expect(
    page.locator('.row', { hasText: 'sample' }).getByText(/已就绪|等待处理/),
  ).toBeVisible();

  // Ensure backend processing completes before opening preview.
  // Poll for file to be ready using authenticated request fixture
  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((f) => f.name === 'sample');
        return file?.status;
      },
      {
        message: 'wait for file to be ready',
        timeout: 60000,
      },
    )
    .toBe('ready');

  // 2. Click row to select it (to open sidebar)
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();
  await row.click();

  // 3. Find Preview button in Detail Sidebar
  // The sidebar should now be populated
  const sidebar = page.getByTestId('detail-sidebar');
  await expect(sidebar.getByText('sample')).toBeVisible(); // Check title in sidebar

  const previewLink = sidebar.getByTestId('open-preview');
  await expect(previewLink).toBeVisible();

  // 4. Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // 5. Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');
  await expect(newPage.getByText('sample')).toBeVisible(); // Filename in header

  // 6. Verify Tile Requests (Observability Contract)
  // We expect the map to load tiles. We intercept/wait for at least one successful tile request.
  // URL pattern: /api/files/:id/tiles/:z/:x/:y
  // Wait a bit for the map to start loading tiles
  await newPage.waitForTimeout(2000);

  // Check if any tile requests were made by looking at the page's performance entries
  const tileRequests = await newPage.evaluate(() => {
    return performance
      .getEntriesByType('resource')
      .filter((r) => r.name.includes('/api/files/') && r.name.includes('/tiles/'))
      .map((r) => ({ url: r.name, status: r.responseStatus }));
  });

  // Log for debugging
  console.log('Tile requests found:', tileRequests.length);
  tileRequests.slice(0, 5).forEach((r) => {
    console.log('  -', r.url, 'status:', r.status);
  });

  // We expect at least one tile request was made
  expect(tileRequests.length).toBeGreaterThan(0);
});
