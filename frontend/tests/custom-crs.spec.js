import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const customCrsDir = path.join(__dirname, '..', '..', 'testdata', 'custom-crs');
const fixturesDir = path.join(__dirname, 'fixtures');

async function waitForFileReady(request, name) {
  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === name);
        return file?.status;
      },
      { message: `wait for ${name} to be ready`, timeout: 60000 },
    )
    .toBe('ready');
}

async function getFileByName(request, name) {
  const response = await request.get('/api/files');
  expect(response.ok()).toBeTruthy();
  const files = await response.json();
  return files.find((item) => item.name === name);
}

async function readTileGridDebugState(previewPage) {
  return previewPage.evaluate(() => {
    const mapElement =
      document.querySelector('[data-testid="preview-map-canvas"]') ||
      document.querySelector('[data-testid="preview-map"]');
    const map = mapElement?.__mapflowPreviewMap || window.__mapflowPreviewMap;
    if (!map) {
      return { hasMap: false, hasDebugLayer: false };
    }

    const layers = map.getLayers().getArray();
    const debugLayer = layers.find((layer) => layer?.get?.('mapflowRole') === 'tile-grid-debug');
    if (!debugLayer) {
      return { hasMap: true, hasDebugLayer: false };
    }

    const source = debugLayer.getSource();
    const tileGrid = source?.getTileGrid?.();
    const projection = source?.getProjection?.();
    const projectionCode =
      projection && typeof projection.getCode === 'function' ? projection.getCode() : projection;
    const extent =
      tileGrid && typeof tileGrid.getExtent === 'function' ? tileGrid.getExtent() : null;
    const origin =
      tileGrid && typeof tileGrid.getOrigin === 'function' ? tileGrid.getOrigin(0) : null;
    const resolutions =
      tileGrid && typeof tileGrid.getResolutions === 'function' ? tileGrid.getResolutions() : null;

    return {
      hasMap: true,
      hasDebugLayer: true,
      visible: debugLayer.getVisible(),
      hasTileGrid: !!tileGrid,
      extent,
      origin,
      resolutionCount: Array.isArray(resolutions) ? resolutions.length : null,
      projectionCode,
    };
  });
}

test.describe('Custom CRS', () => {
  test.beforeEach(async ({ workerServer, request }) => {
    await workerServer.reset();
    await setupTestUser(request);
    await loginUser(request);
  });

  test('upload GeoJSON without CRS and verify preview', async ({ page, request }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_no_crs.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(testFile);

    await expect(
      page
        .locator('.row', { hasText: 'sf_buildings_no_crs' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'sf_buildings_no_crs');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const fileData = files.find((f) => f.name === 'sf_buildings_no_crs');
    expect(fileData).toBeDefined();
    expect(fileData.crsType).toBe('custom');
    expect(fileData.crs).toBeNull();

    const fileId = fileData.id;

    const previewResponse = await request.get(`/api/files/${fileId}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();

    expect(previewData.crsType).toBe('custom');
    expect(previewData.bbox).toBeDefined();
    expect(previewData.bbox).toHaveLength(4);
    expect(previewData.bbox[0]).toBeLessThan(previewData.bbox[2]);
    expect(previewData.bbox[1]).toBeLessThan(previewData.bbox[3]);

    const tileResponse = await request.get(`/api/files/${fileId}/tiles/0/0/0`);
    expect(tileResponse.status()).toBe(200);
    expect(tileResponse.headers()['content-type']).toContain('application/vnd.mapbox-vector-tile');
  });

  test('upload Shapefile with WKT CRS (no EPSG authority)', async ({ page, request }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_custom_wkt.zip');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(testFile);

    await expect(
      page
        .locator('.row', { hasText: 'sf_buildings_custom_wkt' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'sf_buildings_custom_wkt');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const fileData = files.find((f) => f.name === 'sf_buildings_custom_wkt');
    expect(fileData).toBeDefined();
    expect(fileData.crsType).toBe('custom');
    expect(fileData.crs).toBeNull();

    const previewResponse = await request.get(`/api/files/${fileData.id}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();

    expect(previewData.crsType).toBe('custom');
    expect(previewData.bbox).toBeDefined();
  });

  test('custom CRS with named CRS definition', async ({ page, request }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_parks_named_crs.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(testFile);

    await expect(
      page
        .locator('.row', { hasText: 'sf_parks_named_crs' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'sf_parks_named_crs');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const fileData = files.find((f) => f.name === 'sf_parks_named_crs');
    expect(fileData).toBeDefined();
    expect(fileData.crsType).toBe('custom');
    expect(fileData.crs).toBeNull();

    const previewResponse = await request.get(`/api/files/${fileData.id}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();

    expect(previewData.crsType).toBe('custom');
    expect(previewData.bbox).toBeDefined();
  });

  test('custom CRS with negative coordinates', async ({ page, request }) => {
    test.setTimeout(120000);

    const negativeCoordsFile = path.join(customCrsDir, 'negative_coords_test.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(negativeCoordsFile);

    await expect(
      page
        .locator('.row', { hasText: 'negative_coords_test' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'negative_coords_test');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const negativeData = files.find((f) => f.name === 'negative_coords_test');
    expect(negativeData).toBeDefined();
    expect(negativeData.crsType).toBe('custom');
    expect(negativeData.crs).toBeNull();

    const previewResponse = await request.get(`/api/files/${negativeData.id}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();

    expect(previewData.bbox[0]).toBeLessThan(0);
    expect(previewData.bbox[1]).toBeLessThan(0);
  });

  test('custom CRS preview tile grid uses custom tile grid configuration', async ({
    page,
    request,
  }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_no_crs.geojson');

    await page.goto('/');
    await page.getByTestId('file-input').setInputFiles(testFile);

    await expect(
      page
        .locator('.row', { hasText: 'sf_buildings_no_crs' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await waitForFileReady(request, 'sf_buildings_no_crs');

    const fileData = await getFileByName(request, 'sf_buildings_no_crs');
    expect(fileData).toBeDefined();

    const previewMetaResponse = await request.get(`/api/files/${fileData.id}/preview`);
    expect(previewMetaResponse.ok()).toBeTruthy();
    const previewMeta = await previewMetaResponse.json();
    expect(previewMeta.crsType).toBe('custom');
    expect(previewMeta.dataBounds).toHaveLength(4);

    const row = page.locator('.row', { hasText: 'sf_buildings_no_crs' });
    const previewLink = row.getByTestId('preview-link');
    await expect(previewLink).toBeVisible();

    const [previewPage] = await Promise.all([
      page.context().waitForEvent('page'),
      previewLink.click(),
    ]);
    await previewPage.waitForLoadState('networkidle');
    await expect(previewPage.getByText('sf_buildings_no_crs')).toBeVisible();

    await previewPage.getByLabel('Show Tile Grid').check();

    await expect
      .poll(
        async () => {
          const state = await readTileGridDebugState(previewPage);
          return state.visible;
        },
        { message: 'wait for tile grid debug layer to become visible', timeout: 10000 },
      )
      .toBe(true);

    const state = await readTileGridDebugState(previewPage);
    expect(state.hasMap).toBe(true);
    expect(state.hasDebugLayer).toBe(true);
    expect(state.hasTileGrid).toBe(true);
    expect(state.resolutionCount).toBe(21);
    expect(state.projectionCode).toBe(previewMeta.crs || 'CUSTOM_CRS');

    const [minx, miny, maxx, maxy] = previewMeta.dataBounds;
    expect(state.extent[0]).toBeCloseTo(minx, 6);
    expect(state.extent[1]).toBeCloseTo(miny, 6);
    expect(state.extent[2]).toBeCloseTo(maxx, 6);
    expect(state.extent[3]).toBeCloseTo(maxy, 6);
    expect(state.origin[0]).toBeCloseTo(minx, 6);
    expect(state.origin[1]).toBeCloseTo(maxy, 6);

    await previewPage.close();
  });

  test('standard CRS preview tile grid toggle still works', async ({ page, request }) => {
    test.setTimeout(120000);

    const testFile = path.join(fixturesDir, 'sample.geojson');

    await page.goto('/');
    await page.getByTestId('file-input').setInputFiles(testFile);

    await expect(
      page.locator('.row', { hasText: 'sample' }).getByTestId(/status-ready|status-processing/),
    ).toBeVisible();
    await waitForFileReady(request, 'sample');

    const row = page.locator('.row', { hasText: 'sample' });
    const previewLink = row.getByTestId('preview-link');
    await expect(previewLink).toBeVisible();

    const [previewPage] = await Promise.all([
      page.context().waitForEvent('page'),
      previewLink.click(),
    ]);
    await previewPage.waitForLoadState('networkidle');
    await expect(previewPage.getByText('sample')).toBeVisible();

    await previewPage.getByLabel('Show Tile Grid').check();
    await expect
      .poll(
        async () => {
          const state = await readTileGridDebugState(previewPage);
          return state.visible;
        },
        {
          message: 'wait for standard CRS tile grid debug layer to become visible',
          timeout: 10000,
        },
      )
      .toBe(true);

    const state = await readTileGridDebugState(previewPage);
    expect(state.hasMap).toBe(true);
    expect(state.hasDebugLayer).toBe(true);

    await previewPage.close();
  });

  test('update CRS with EPSG URN (4490) and verify docs tile requests', async ({
    page,
    context,
    request,
    workerServer,
  }) => {
    test.setTimeout(120000);

    const filePath = path.join(customCrsDir, 'epsg4490_urn.geojson');
    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(filePath);

    await expect(
      page
        .locator('.row', { hasText: 'epsg4490_urn' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'epsg4490_urn');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const fileData = files.find((f) => f.name === 'epsg4490_urn');
    expect(fileData).toBeDefined();

    const updateCrsResponse = await request.fetch(`/api/files/${fileData.id}/crs`, {
      method: 'PUT',
      data: { crs: 'urn:ogc:def:crs:EPSG::4490' },
    });
    expect(updateCrsResponse.ok()).toBeTruthy();
    const updateData = await updateCrsResponse.json();
    expect(updateData.crs).toBe('EPSG:4490');
    expect(updateData.crsType).toBe('standard');

    const previewResponse = await request.get(`/api/files/${fileData.id}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();
    expect(previewData.crsType).toBe('standard');
    expect(previewData.bbox).toBeDefined();
    expect(previewData.bbox).toHaveLength(4);
    expect(previewData.bbox[0]).toBeLessThanOrEqual(previewData.bbox[2]);
    expect(previewData.bbox[1]).toBeLessThanOrEqual(previewData.bbox[3]);

    const publishResponse = await request.post(`/api/files/${fileData.id}/publish`, {
      data: { slug: 'epsg4490-urn-docs' },
    });
    expect(publishResponse.ok()).toBeTruthy();

    const metaResponse = await request.get('/tiles/epsg4490-urn-docs/meta');
    expect(metaResponse.ok()).toBeTruthy();
    const metaData = await metaResponse.json();
    expect(metaData.crs).toBe('EPSG:4490');
    expect(metaData.crsType).toBe('standard');
    expect(metaData.bbox).toBeDefined();
    expect(metaData.bbox).toHaveLength(4);

    const publicContext = await context.browser().newContext();
    const docsPage = await publicContext.newPage();
    await docsPage.goto(`${workerServer.url}/tiles/epsg4490-urn-docs/docs`);
    await docsPage.waitForLoadState('networkidle');
    await expect(docsPage.getByText('Live Preview (Public Endpoint)')).toBeVisible();

    await expect
      .poll(
        async () => {
          return docsPage.evaluate(() => {
            return performance
              .getEntriesByType('resource')
              .filter(
                (r) => r.name.includes('/tiles/epsg4490-urn-docs/') && !r.name.includes('/meta'),
              )
              .map((r) => r.responseStatus)
              .filter((status) => status === 200 || status === 204).length;
          });
        },
        { message: 'wait for docs page tile requests', timeout: 10000 },
      )
      .toBeGreaterThan(0);

    const embedPage = await publicContext.newPage();
    await embedPage.goto(`${workerServer.url}/tiles/epsg4490-urn-docs/embed`);
    await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
    await expect(embedPage.getByText('Back to Files')).toHaveCount(0);

    await expect
      .poll(
        async () => {
          return embedPage.evaluate(() => {
            return performance
              .getEntriesByType('resource')
              .filter(
                (resource) =>
                  resource.name.includes('/tiles/epsg4490-urn-docs/') &&
                  !resource.name.includes('/meta'),
              )
              .map((resource) => resource.responseStatus)
              .filter((status) => status === 200 || status === 204).length;
          });
        },
        { message: 'wait for embed page tile requests', timeout: 10000 },
      )
      .toBeGreaterThan(0);

    await publicContext.close();
  });

  test('custom CRS preview hides OSM basemap toggle and does not request OSM tiles', async ({
    page,
    request,
  }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_no_crs.geojson');

    await page.goto('/');
    await page.getByTestId('file-input').setInputFiles(testFile);

    await expect(
      page
        .locator('.row', { hasText: 'sf_buildings_no_crs' })
        .getByTestId(/status-ready|status-processing/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'sf_buildings_no_crs');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const row = page.locator('.row', { hasText: 'sf_buildings_no_crs' });
    const previewLink = row.getByTestId('preview-link');

    let osmRequestCount = 0;
    await page.context().route('https://tile.openstreetmap.org/**', async (route) => {
      osmRequestCount += 1;
      await route.abort();
    });

    const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);
    await newPage.waitForLoadState('networkidle');

    await expect(newPage.getByLabel('Show OSM Basemap')).toHaveCount(0);
    await newPage.waitForTimeout(1500);
    expect(osmRequestCount).toBe(0);
  });

  test('custom CRS public docs and embed honor published maxZoom', async ({
    page,
    request,
    context,
    workerServer,
  }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_no_crs.geojson');

    await page.goto('/');
    await page.getByTestId('file-input').setInputFiles(testFile);
    await waitForFileReady(request, 'sf_buildings_no_crs');

    const fileData = await getFileByName(request, 'sf_buildings_no_crs');
    expect(fileData).toBeDefined();

    const publishResponse = await request.post(`/api/files/${fileData.id}/publish`, {
      data: { slug: 'custom-crs-maxzoom-1', minZoom: 0, maxZoom: 1 },
    });
    expect(publishResponse.ok()).toBeTruthy();

    const publicContext = await context.browser().newContext();

    const docsPage = await publicContext.newPage();
    await docsPage.goto(`${workerServer.url}/tiles/custom-crs-maxzoom-1/docs`);
    await expect(docsPage.locator('pre code')).toContainText('maxZoom = 1');

    const embedPage = await publicContext.newPage();
    await embedPage.goto(`${workerServer.url}/tiles/custom-crs-maxzoom-1/embed`);
    await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
    await expect
      .poll(
        async () => {
          return embedPage.evaluate(() => {
            const map = window.__mapflowPublicTileMap;
            const view = map?.getView?.();
            return view ? view.getMaxZoom() : null;
          });
        },
        { message: 'wait for custom CRS embed maxZoom', timeout: 10000 },
      )
      .toBe(1);

    await publicContext.close();
  });

  test('custom CRS embed supports published maxZoom above 20', async ({
    page,
    request,
    context,
    workerServer,
  }) => {
    test.setTimeout(120000);

    const testFile = path.join(customCrsDir, 'sf_buildings_no_crs.geojson');

    await page.goto('/');
    await page.getByTestId('file-input').setInputFiles(testFile);
    await waitForFileReady(request, 'sf_buildings_no_crs');

    const fileData = await getFileByName(request, 'sf_buildings_no_crs');
    expect(fileData).toBeDefined();

    const publishResponse = await request.post(`/api/files/${fileData.id}/publish`, {
      data: { slug: 'custom-crs-maxzoom-22', minZoom: 0, maxZoom: 22 },
    });
    expect(publishResponse.ok()).toBeTruthy();

    const publicContext = await context.browser().newContext();
    const embedPage = await publicContext.newPage();
    await embedPage.goto(`${workerServer.url}/tiles/custom-crs-maxzoom-22/embed`);
    await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
    await expect
      .poll(
        async () => {
          return embedPage.evaluate(() => {
            const map = window.__mapflowPublicTileMap;
            const view = map?.getView?.();
            const layer = map?.getLayers?.().item(0);
            const source = layer?.getSource?.();
            const tileGrid = source?.getTileGrid?.();
            const resolutions =
              tileGrid && typeof tileGrid.getResolutions === 'function'
                ? tileGrid.getResolutions()
                : null;
            return view && Array.isArray(resolutions)
              ? { maxZoom: view.getMaxZoom(), resolutionCount: resolutions.length }
              : null;
          });
        },
        { message: 'wait for custom CRS embed tile grid max zoom', timeout: 10000 },
      )
      .toMatchObject({ maxZoom: 22, resolutionCount: 23 });

    await publicContext.close();
  });
});
