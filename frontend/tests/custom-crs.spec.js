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

  test('upload custom CRS file and verify API behavior', async ({ page, request }) => {
    test.setTimeout(120000);

    const customCrsFile = path.join(customCrsDir, 'no_crs_test.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(customCrsFile);

    await expect(
      page.locator('.row', { hasText: 'no_crs_test' }).getByText(/已就绪|等待处理/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'no_crs_test');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const customCrsData = files.find((f) => f.name === 'no_crs_test');
    expect(customCrsData).toBeDefined();
    expect(customCrsData.crsType).toBe('custom');
    expect(customCrsData.crs).toBeNull();

    const fileId = customCrsData.id;

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

    const previewResponse = await request.get(`/api/files/${negativeData.id}/preview`);
    expect(previewResponse.ok()).toBeTruthy();
    const previewData = await previewResponse.json();

    expect(previewData.bbox[0]).toBeLessThan(0);
    expect(previewData.bbox[1]).toBeLessThan(0);
  });

  test('custom CRS with named CRS definition', async ({ page, request }) => {
    test.setTimeout(120000);

    const complexFile = path.join(customCrsDir, 'complex_custom_crs.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(complexFile);

    await expect(
      page.locator('.row', { hasText: 'complex_custom_crs' }).getByText(/已就绪|等待处理/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'complex_custom_crs');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const complexData = files.find((f) => f.name === 'complex_custom_crs');
    expect(complexData).toBeDefined();
    expect(complexData.crsType).toBe('custom');
    expect(complexData.crs).toBeNull();
  });

  test('custom CRS preview page loads tiles', async ({ page, request }) => {
    test.setTimeout(120000);

    const customCrsFile = path.join(customCrsDir, 'simple_custom_crs.geojson');

    await page.goto('/');

    const input = page.getByTestId('file-input');
    await input.setInputFiles(customCrsFile);

    await expect(
      page.locator('.row', { hasText: 'simple_custom_crs' }).getByText(/已就绪|等待处理/),
    ).toBeVisible();

    await expect
      .poll(
        async () => {
          const response = await request.get('/api/files');
          if (!response.ok()) return null;
          const files = await response.json();
          const file = files.find((f) => f.name === 'simple_custom_crs');
          return file?.status;
        },
        { message: 'wait for file to be ready', timeout: 60000 },
      )
      .toBe('ready');

    const row = page.locator('.row', { hasText: 'simple_custom_crs' });
    await expect(row).toBeVisible();

    const previewLink = row.getByRole('link', { name: '查看' });
    await expect(previewLink).toBeVisible();

    const [newPage] = await Promise.all([page.context().waitForEvent('page'), previewLink.click()]);

    await newPage.waitForLoadState('networkidle');

    expect(newPage.url()).toContain('/preview/');
    await expect(newPage.getByText('simple_custom_crs')).toBeVisible();

    const filesResponse = await request.get('/api/files');
    const files = await filesResponse.json();
    const simpleData = files.find((f) => f.name === 'simple_custom_crs');
    const tileResponse = await request.get(`/api/files/${simpleData.id}/tiles/0/0/0`);
    expect([200, 204]).toContain(tileResponse.status());
  });
});
