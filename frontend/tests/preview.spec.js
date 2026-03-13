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

function normalizeColor(value) {
  return String(value || '')
    .toLowerCase()
    .replace(/\s+/g, '');
}

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  // Initialize and login test user
  await setupTestUser(request);
  await loginUser(request);
});

test('click preview opens new tab with map', async ({ page, workerServer, request }) => {
  test.setTimeout(120000); // Increase timeout to 120s for this test
  await enablePreviewE2EHooks(page);
  // 1. Upload a file via UI (since we can't easily seed DuckDB from here without a tool)
  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');

  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  // Wait for upload to complete (could be 'ready' or 'processing' depending on timing)
  // We accept either, but ideally we want 'ready' to ensure processing is done for preview
  await expect(
    page
      .locator('.row', { hasText: 'sample' })
      .filter({ has: page.getByTestId(/status-ready|status-processing/) }),
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

  // 2. Find and click preview link in file row action area
  const row = page.locator('.row', { hasText: 'sample' });
  await expect(row).toBeVisible();

  const previewLink = row.getByTestId('preview-link');
  await expect(previewLink).toBeVisible();

  // 3. Click preview link and wait for new page
  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

  await newPage.waitForLoadState('networkidle');

  // 5. Verify URL and Content on new page
  expect(newPage.url()).toContain('/preview/');
  await expect(newPage.getByText('sample')).toBeVisible(); // Filename in header

  // 6. Verify Tile Requests (Observability Contract)
  // Poll for at least one tile request to complete
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
});

test('click feature switches highlight style immediately', async ({ page, request }) => {
  test.setTimeout(120000);
  await enablePreviewE2EHooks(page);

  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');
  await page.getByTestId('file-input').setInputFiles(geojsonPath);

  await expect(
    page
      .locator('.row', { hasText: 'sample' })
      .filter({ has: page.getByTestId(/status-ready|status-processing/) }),
  ).toBeVisible();

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        return files.find((f) => f.name === 'sample')?.status ?? null;
      },
      { message: 'wait for file to be ready', timeout: 60000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' });
  const previewLink = row.getByTestId('preview-link');
  await expect(previewLink).toBeVisible();

  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);
  await newPage.waitForLoadState('networkidle');

  await expect
    .poll(
      async () => {
        const tileRequests = await newPage.evaluate(() => {
          return performance
            .getEntriesByType('resource')
            .filter((r) => r.name.includes('/api/files/') && r.name.includes('/tiles/')).length;
        });
        return tileRequests;
      },
      { message: 'wait for tile requests', timeout: 10000 },
    )
    .toBeGreaterThan(0);

  const mapCanvas = newPage.getByTestId('preview-map-canvas');
  await expect(mapCanvas).toBeVisible();

  const box = await mapCanvas.boundingBox();
  expect(box).not.toBeNull();
  if (!box) {
    throw new Error('preview-map-canvas has no bounding box');
  }

  await newPage.mouse.click(box.x + box.width / 2, box.y + box.height / 2);

  await expect
    .poll(
      async () =>
        await newPage.evaluate(() => {
          return window.__MAPFLOW_PREVIEW_TEST__?.getHighlightDebug?.()?.selectedFid ?? null;
        }),
      { message: 'wait for selected feature fid', timeout: 10000 },
    )
    .not.toBeNull();

  await expect(newPage.getByTestId('feature-inspector')).toBeVisible();
  await expect(newPage.getByTestId('feature-inspector-title')).toBeVisible();

  const highlightDebug = await newPage.evaluate(() =>
    window.__MAPFLOW_PREVIEW_TEST__.getHighlightDebug(),
  );
  expect(typeof highlightDebug.selectedFid).toBe('number');

  const selectedStyle = await newPage.evaluate(() => {
    const api = window.__MAPFLOW_PREVIEW_TEST__;
    const selectedFid = api.getHighlightDebug().selectedFid;
    return api.getStyleForFid(selectedFid);
  });
  const nonSelectedStyle = await newPage.evaluate(() => {
    const api = window.__MAPFLOW_PREVIEW_TEST__;
    const selectedFid = api.getHighlightDebug().selectedFid;
    return api.getStyleForFid(selectedFid + 1);
  });

  expect(normalizeColor(selectedStyle.fill)).toBe('rgba(255,200,0,0.7)');
  expect(normalizeColor(selectedStyle.strokeColor)).toBe('#ffc800');
  expect(selectedStyle.strokeWidth).toBe(4);

  expect(normalizeColor(nonSelectedStyle.fill)).toBe('rgba(0,128,255,0.6)');
  expect(normalizeColor(nonSelectedStyle.strokeColor)).toBe('#0080ff');
  expect(nonSelectedStyle.strokeWidth).toBe(2);
});

test('standard CRS preview shows OSM basemap toggle and requests OSM tiles when enabled', async ({
  page,
  request,
}) => {
  test.setTimeout(120000);

  const fixturesDir = path.join(__dirname, 'fixtures');
  const geojsonPath = path.join(fixturesDir, 'sample.geojson');

  await page.goto('/');
  await page.getByTestId('file-input').setInputFiles(geojsonPath);

  await expect(
    page
      .locator('.row', { hasText: 'sample' })
      .filter({ has: page.getByTestId(/status-ready|status-processing/) }),
  ).toBeVisible();

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        return files.find((f) => f.name === 'sample')?.status ?? null;
      },
      { message: 'wait for file to be ready', timeout: 60000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' });
  const previewLink = row.getByTestId('preview-link');
  await expect(previewLink).toBeVisible();

  let osmRequestCount = 0;
  await page.context().route('https://tile.openstreetmap.org/**', async (route) => {
    osmRequestCount += 1;
    await route.abort();
  });

  const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);
  await newPage.waitForLoadState('networkidle');

  const osmToggle = newPage.getByTestId('preview-osm-basemap-toggle');
  await expect(osmToggle).toBeVisible();
  await expect(osmToggle).not.toBeChecked();

  await osmToggle.check();

  await expect
    .poll(() => osmRequestCount, { message: 'wait for OSM tile requests', timeout: 10000 })
    .toBeGreaterThan(0);
});
