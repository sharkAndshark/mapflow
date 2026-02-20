import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const customCrsDir = path.join(__dirname, '..', '..', 'testdata', 'custom-crs');

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
      page.locator('.row', { hasText: 'sf_buildings_no_crs' }).getByText(/已就绪|等待处理/),
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
      page.locator('.row', { hasText: 'sf_buildings_custom_wkt' }).getByText(/已就绪|等待处理/),
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
      page.locator('.row', { hasText: 'sf_parks_named_crs' }).getByText(/已就绪|等待处理/),
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
      page.locator('.row', { hasText: 'negative_coords_test' }).getByText(/已就绪|等待处理/),
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
});
