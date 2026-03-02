import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function enablePreviewE2EHooks(page) {
  await page.context().addInitScript(() => {
    window.__MAPFLOW_E2E__ = true;
  });
}

async function waitForTileRequests(newPage) {
  await expect
    .poll(
      async () => {
        const tileRequests = await newPage.evaluate(() => {
          return performance
            .getEntriesByType('resource')
            .filter((r) => r.name.includes('/api/files/') && r.name.includes('/tiles/'))
            .map((r) => ({ url: r.name, status: r.responseStatus }));
        });
        return tileRequests.length;
      },
      { message: 'wait for tile requests', timeout: 10000 },
    )
    .toBeGreaterThan(0);
}

async function getPreviewZoomState(newPage) {
  return await newPage.evaluate(() => window.__MAPFLOW_PREVIEW_TEST__?.getZoomState?.() ?? null);
}

async function clickZoomButtonUntilDisabled(newPage, selector, maxClicks = 30) {
  const button = newPage.locator(selector);
  await expect(button).toBeVisible();

  for (let i = 0; i < maxClicks; i += 1) {
    const disabled = await button.evaluate((el) => el.classList.contains('ol-disabled'));
    if (disabled) {
      return;
    }
    await button.click();
  }
}

async function getTileZoomStats(newPage, fileId) {
  return await newPage.evaluate((fid) => {
    const marker = `/api/files/${fid}/tiles/`;
    const zooms = performance
      .getEntriesByType('resource')
      .map((entry) => entry.name)
      .filter((url) => url.includes(marker))
      .map((url) => {
        const rest = url.slice(url.indexOf(marker) + marker.length);
        const z = Number.parseInt(rest.split('/')[0], 10);
        return Number.isFinite(z) ? z : null;
      })
      .filter((z) => z != null);

    if (zooms.length === 0) {
      return { count: 0, min: null, max: null };
    }
    return {
      count: zooms.length,
      min: Math.min(...zooms),
      max: Math.max(...zooms),
    };
  }, fileId);
}

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  // Initialize and login test user
  await setupTestUser(request);
  await loginUser(request);
});

test('mbtiles file has zoom limits', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test
  await enablePreviewE2EHooks(page);

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

  // Click "查看" link in file row action area
  const row = page.locator('.row', { hasText: /sample/ });
  await expect(row).toBeVisible();

  const previewLink = row.getByRole('link', { name: '查看' });
  await expect(previewLink).toBeVisible();

  // Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');

  await waitForTileRequests(newPage);

  // Wait until preview test hook is available and assert view limits match metadata.
  await expect
    .poll(() => getPreviewZoomState(newPage), {
      message: 'wait for preview zoom hook',
      timeout: 5000,
    })
    .not.toBeNull();
  const zoomState = await getPreviewZoomState(newPage);
  expect(zoomState.minZoom).toBe(previewData.minZoom);
  expect(zoomState.maxZoom).toBe(previewData.maxZoom);

  const initialTileStats = await getTileZoomStats(newPage, fileId);
  expect(initialTileStats.count).toBeGreaterThan(0);

  // Verify actual UI zoom interactions are bounded by frontend min/max via observed tile z.
  await clickZoomButtonUntilDisabled(newPage, '.ol-zoom-in', 10);
  const highTileStats = await getTileZoomStats(newPage, fileId);
  expect(highTileStats.count).toBeGreaterThanOrEqual(initialTileStats.count);
  expect(highTileStats.max).toBeLessThanOrEqual(previewData.maxZoom);

  await clickZoomButtonUntilDisabled(newPage, '.ol-zoom-out', 20);
  const lowTileStats = await getTileZoomStats(newPage, fileId);
  expect(lowTileStats.count).toBeGreaterThanOrEqual(highTileStats.count);
  expect(lowTileStats.min).toBeGreaterThanOrEqual(previewData.minZoom);
});

test('dynamic table has no zoom limits', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test
  await enablePreviewE2EHooks(page);

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

  // Check preview metadata - dynamic data uses fixed zoom range (0, 22)
  const previewResponse = await request.get(`/api/files/${fileId}/preview`);
  expect(previewResponse.ok()).toBeTruthy();
  const previewData = await previewResponse.json();

  // Verify dynamic data uses fixed preview zoom range (0, 22)
  expect(previewData.minZoom).toBe(0);
  expect(previewData.maxZoom).toBe(22);

  // Click "查看" link in file row action area
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();

  const previewLink = row.getByRole('link', { name: '查看' });
  await expect(previewLink).toBeVisible();

  // Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');

  await waitForTileRequests(newPage);

  await expect
    .poll(() => getPreviewZoomState(newPage), {
      message: 'wait for preview zoom hook',
      timeout: 5000,
    })
    .not.toBeNull();
  const zoomState = await getPreviewZoomState(newPage);
  expect(zoomState.minZoom).toBe(0);
  expect(zoomState.maxZoom).toBe(22);

  const initialTileStats = await getTileZoomStats(newPage, fileId);
  expect(initialTileStats.count).toBeGreaterThan(0);

  await clickZoomButtonUntilDisabled(newPage, '.ol-zoom-in', 30);
  const highTileStats = await getTileZoomStats(newPage, fileId);
  expect(highTileStats.count).toBeGreaterThanOrEqual(initialTileStats.count);
  expect(highTileStats.max).toBeLessThanOrEqual(22);

  await clickZoomButtonUntilDisabled(newPage, '.ol-zoom-out', 40);
  const lowTileStats = await getTileZoomStats(newPage, fileId);
  expect(lowTileStats.count).toBeGreaterThanOrEqual(highTileStats.count);
  expect(lowTileStats.min).toBeGreaterThanOrEqual(0);
});
