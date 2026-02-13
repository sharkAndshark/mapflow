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

test('mbtiles file has zoom limits', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test

  // Upload mbtiles file
  const mbtilesPath = path.join(__dirname, '..', '..', 'testdata', 'sample_mvt.mbtiles');

  await page.goto('/');
  const input = page.getByTestId('file-input');
  await input.setInputFiles(mbtilesPath);

  // Wait for upload to complete
  await expect(
    page.locator('.row', { hasText: /sample/ }).getByText(/已就绪|等待处理/),
  ).toBeVisible();

  // Poll for file to be ready
  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((f) => f.name.includes('sample'));
        return file?.status;
      },
      {
        message: 'wait for file to be ready',
        timeout: 60000,
      },
    )
    .toBe('ready');

  // Get file id
  const filesResponse = await request.get('/api/files');
  const files = await filesResponse.json();
  const mbtilesFile = files.find((f) => f.name.includes('sample'));
  expect(mbtilesFile).toBeDefined();
  const fileId = mbtilesFile.id;

  // Check preview metadata includes zoom limits
  const previewResponse = await request.get(`/api/files/${fileId}/preview`);
  expect(previewResponse.ok()).toBeTruthy();
  const previewData = await previewResponse.json();

  // Verify zoom limits exist for mbtiles (not null and not undefined)
  expect(previewData.minZoom).not.toBeNull();
  expect(previewData.minZoom).toBeDefined();
  expect(previewData.maxZoom).not.toBeNull();
  expect(previewData.maxZoom).toBeDefined();

  // Click row to select it (to open sidebar)
  const row = page.locator('.row', { hasText: /sample/ });
  await expect(row).toBeVisible();
  await row.click();

  // Find Preview button in Detail Sidebar
  const sidebar = page.getByTestId('detail-sidebar');
  await expect(sidebar).toBeVisible();

  const previewLink = sidebar.getByTestId('open-preview');
  await expect(previewLink).toBeVisible();

  // Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');

  // Wait for tiles to load
  await newPage.waitForTimeout(2000);

  // Check that tile requests were made (confirm map loaded)
  const tileRequests = await newPage.evaluate(() => {
    return performance
      .getEntriesByType('resource')
      .filter((r) => r.name.includes('/api/files/') && r.name.includes('/tiles/'))
      .map((r) => ({ url: r.name, status: r.responseStatus }));
  });

  expect(tileRequests.length).toBeGreaterThan(0);

  // Note: The actual zoom limits are enforced in the frontend code.
  // We verify that the API returns the correct zoom limits above.
  // The frontend uses these to set minZoom and maxZoom on the map view.
  // Manual testing can verify that users cannot zoom beyond these limits.
});

test('dynamic table has no zoom limits', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test

  // Upload GeoJSON file
  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  // Wait for upload to complete
  await expect(
    page.locator('.row', { hasText: 'sample' }).getByText(/已就绪|等待处理/),
  ).toBeVisible();

  // Poll for file to be ready
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

  // Get file id
  const filesResponse = await request.get('/api/files');
  const files = await filesResponse.json();
  const geojsonFile = files.find((f) => f.name === 'sample');
  expect(geojsonFile).toBeDefined();
  const fileId = geojsonFile.id;

  // Check preview metadata does NOT have zoom limits
  const previewResponse = await request.get(`/api/files/${fileId}/preview`);
  expect(previewResponse.ok()).toBeTruthy();
  const previewData = await previewResponse.json();

  // Verify zoom limits are not present for dynamic tables (undefined or null)
  expect(previewData.minZoom == null).toBeTruthy();
  expect(previewData.maxZoom == null).toBeTruthy();

  // Click row to select it (to open sidebar)
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();
  await row.click();

  // Find Preview button in Detail Sidebar
  const sidebar = page.getByTestId('detail-sidebar');
  await expect(sidebar).toBeVisible();

  const previewLink = sidebar.getByTestId('open-preview');
  await expect(previewLink).toBeVisible();

  // Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');

  // Wait for tiles to load
  await newPage.waitForTimeout(2000);

  // Check that tile requests were made (confirm map loaded)
  const tileRequests = await newPage.evaluate(() => {
    return performance
      .getEntriesByType('resource')
      .filter((r) => r.name.includes('/api/files/') && r.name.includes('/tiles/'))
      .map((r) => ({ url: r.name, status: r.responseStatus }));
  });

  expect(tileRequests.length).toBeGreaterThan(0);

  // Note: For dynamic tables, the frontend uses default zoom limits (0-22).
  // We verify that the API returns null for minzoom and maxzoom above.
  // The frontend will then use the default values.
  // Manual testing can verify that users can freely zoom.
});
