import { test, expect } from './fixtures';
import fs from 'fs/promises';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const geojsonPath = path.join(fixturesDir, 'sample.geojson');
const pmtilesPath = path.join(fixturesDir, 'sample.pmtiles');

async function readPmtilesHeaderSummary(filePath) {
  const buffer = await fs.readFile(filePath);
  return {
    tileType: buffer.readUInt8(99),
    minZoom: buffer.readUInt8(100),
    maxZoom: buffer.readUInt8(101),
  };
}

function tileTypeLabel(tileType) {
  switch (tileType) {
    case 1:
      return 'MVT';
    case 2:
      return 'PNG';
    case 3:
      return 'JPEG';
    case 4:
      return 'WEBP';
    case 5:
      return 'AVIF';
    default:
      return 'MVT';
  }
}

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  await setupTestUser(request);
  await loginUser(request);
});

test('publish flow: upload file, publish with custom slug, access public tiles', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();

  // Click the row to select it and show detail sidebar
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();

  // Switch to Publish tab
  await sidebar.getByText('Publish', { exact: true }).click();

  // Click publish button in sidebar
  const publishButton = sidebar.getByText('发布', { exact: true });
  await expect(publishButton).toBeVisible();
  await publishButton.click();

  // Fill in custom slug in the expanded publish form
  const slugInput = sidebar.getByTestId('publish-slug-input');
  await slugInput.fill('my-custom-map');

  const confirmButton = sidebar.getByText('确认发布');
  await expect(confirmButton).toBeEnabled();
  await confirmButton.click();

  // Wait for publish to complete - should show "已发布" status
  await expect(sidebar.getByText('已发布')).toBeVisible();
  await expect(sidebar.getByText('复制地址')).toBeVisible();
  await expect(sidebar.getByText('取消发布')).toBeVisible();

  await sidebar.getByText('嵌入代码').click();
  await expect(sidebar.locator('.iframe-code-preview')).toContainText('/tiles/my-custom-map/embed');
  await expect(sidebar.locator('iframe[title="MapFlow embed preview"]')).toHaveAttribute(
    'src',
    '/tiles/my-custom-map/embed',
  );

  // Copy public URL (click the button, but don't verify clipboard in test environment)
  const copyButton = sidebar.getByText('复制地址');
  await copyButton.click();

  // Wait for public tile endpoint to be accessible and verify response
  const publicContext = await context.browser().newContext();
  const publicRequest = publicContext.request;
  await expect
    .poll(
      async () => {
        const response = await publicRequest.get(`${workerServer.url}/tiles/my-custom-map/0/0/0`);
        return response.status();
      },
      { message: 'wait for public tile to be accessible', timeout: 10000 },
    )
    .toBe(200);

  const response = await publicRequest.get(`${workerServer.url}/tiles/my-custom-map/0/0/0`);
  expect(response.headers()['content-type']).toContain('application/vnd.mapbox-vector-tile');
  expect(response.headers()['cache-control']).toContain('public, max-age=300');

  const embedPage = await publicContext.newPage();
  await embedPage.goto(`${workerServer.url}/tiles/my-custom-map/embed`);
  await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
  await expect(embedPage.getByText('Back to Files')).toHaveCount(0);
  await expect
    .poll(
      async () => {
        return embedPage.evaluate((publicSlug) => {
          return performance
            .getEntriesByType('resource')
            .filter(
              (resource) =>
                resource.name.includes(`/tiles/${publicSlug}/`) && !resource.name.includes('/meta'),
            )
            .map((resource) => resource.responseStatus)
            .filter((status) => status === 200 || status === 204).length;
        }, 'my-custom-map');
      },
      { message: 'wait for embed page tile requests', timeout: 10000 },
    )
    .toBeGreaterThan(0);

  const docsPage = await publicContext.newPage();
  await docsPage.route('**/tiles/my-custom-map/meta', async (route) => {
    const response = await route.fetch();
    const data = await response.json();
    delete data.viewerUrl;
    await route.fulfill({
      response,
      contentType: 'application/json',
      body: JSON.stringify(data),
    });
  });
  await docsPage.goto(`${workerServer.url}/tiles/my-custom-map/docs`);
  await expect(docsPage.getByText('Embed URL')).toBeVisible();
  await expect(
    docsPage.locator('code').filter({ hasText: '/tiles/my-custom-map/embed' }).first(),
  ).toBeVisible();
  await expect(docsPage.getByText('undefined')).toHaveCount(0);

  await publicContext.close();

  // Test unpublish
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();

  const readyRow = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: '已就绪' })
    .first();
  await expect(readyRow).toBeVisible();
  await readyRow.click();

  const readySidebar = page.locator('.detail-sidebar');
  // Switch to Publish tab
  await readySidebar.getByText('Publish', { exact: true }).click();
  await expect(readySidebar.getByText('已发布')).toBeVisible();

  page.once('dialog', (dialog) => dialog.accept());
  const unpublishButton = readySidebar.getByText('取消发布');
  await unpublishButton.click();

  // Should show publish button again
  await expect(readySidebar.getByText('发布', { exact: true })).toBeVisible();
  await expect(readySidebar.getByText('取消发布')).not.toBeVisible();

  const anonContext = await context.browser().newContext();
  const errorResponse = await anonContext.request.get(
    `${workerServer.url}/tiles/my-custom-map/0/0/0`,
  );
  expect(errorResponse.status()).toBe(404);
  await anonContext.close();
});

test('public embed page shows a visible error for missing slug', async ({
  context,
  workerServer,
}) => {
  const publicContext = await context.browser().newContext();
  const embedPage = await publicContext.newPage();

  await embedPage.goto(`${workerServer.url}/tiles/nonexistent-embed-slug/embed`);

  await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
  await expect(embedPage.locator('.error-alert')).toContainText('Public tile not found');

  await publicContext.close();
});

test('publish PMTiles file exposes working iframe embed', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  await page.getByTestId('file-input').setInputFiles(pmtilesPath);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === 'sample');
        return file?.status;
      },
      { message: 'wait for PMTiles upload to be ready', timeout: 10000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();
  await sidebar.getByText('Publish', { exact: true }).click();
  await sidebar.getByText('发布', { exact: true }).click();

  const slugInput = sidebar.getByTestId('publish-slug-input');
  await slugInput.fill('my-pmtiles');
  await expect(sidebar.locator('.form-value.code')).toContainText('/tiles/my-pmtiles');
  await expect(sidebar.locator('.form-value.code')).not.toContainText('{z}/{x}/{y}');
  const publishZoomInputs = sidebar.locator('input[type="number"]');
  await publishZoomInputs.nth(0).fill('1');
  await publishZoomInputs.nth(1).fill('1');
  await sidebar.getByText('确认发布').click();

  await expect(sidebar.getByText('已发布')).toBeVisible();
  await expect(sidebar.locator('.form-value.code')).toContainText('/tiles/my-pmtiles');
  await sidebar.getByText('嵌入代码').click();
  await expect(sidebar.locator('.iframe-code-preview')).toContainText('/tiles/my-pmtiles/embed');
  await expect(sidebar.locator('iframe[title="MapFlow embed preview"]')).toHaveAttribute(
    'src',
    '/tiles/my-pmtiles/embed',
  );

  const publicContext = await context.browser().newContext();
  const headResponse = await publicContext.request.head(`${workerServer.url}/tiles/my-pmtiles`);
  expect(headResponse.status()).toBe(200);

  const metaResponse = await publicContext.request.get(`${workerServer.url}/tiles/my-pmtiles/meta`);
  expect(metaResponse.ok()).toBeTruthy();
  const metaJson = await metaResponse.json();
  expect(metaJson.viewerUrl).toBe('/tiles/my-pmtiles/embed');

  const embedPage = await publicContext.newPage();
  await embedPage.goto(`${workerServer.url}/tiles/my-pmtiles/embed`);
  await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();
  await expect(embedPage.locator('.error-alert')).toHaveCount(0);
  await expect
    .poll(
      async () => {
        return embedPage.evaluate(() => {
          const map = window.__mapflowPublicTileMap;
          const view = map?.getView?.();
          return view
            ? { minZoom: view.getMinZoom(), maxZoom: view.getMaxZoom(), zoom: view.getZoom() }
            : null;
        });
      },
      { message: 'wait for PMTiles embed view to initialize', timeout: 10000 },
    )
    .toMatchObject({ minZoom: 1, maxZoom: 1 });
  await expect
    .poll(
      async () => {
        return embedPage.evaluate(() => {
          return performance
            .getEntriesByType('resource')
            .filter((resource) => resource.name.includes('/tiles/my-pmtiles'))
            .map((resource) => resource.responseStatus)
            .filter((status) => status === 200 || status === 206).length;
        });
      },
      { message: 'wait for PMTiles range requests', timeout: 10000 },
    )
    .toBeGreaterThan(0);

  const docsPage = await publicContext.newPage();
  await docsPage.goto(`${workerServer.url}/tiles/my-pmtiles/docs`);
  await expect(docsPage.locator('pre code')).toContainText('const publishedMinZoom = 1;');
  await expect(docsPage.locator('pre code')).toContainText('const publishedMaxZoom = 1;');

  await publicContext.close();
});

test('PMTiles docs code falls back to archive zoom bounds when publish zoom is unset', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  const pmtilesHeader = await readPmtilesHeaderSummary(pmtilesPath);

  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  await page.getByTestId('file-input').setInputFiles(pmtilesPath);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === 'sample');
        return file?.status;
      },
      { message: 'wait for PMTiles upload to be ready', timeout: 10000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await sidebar.getByText('Publish', { exact: true }).click();
  await sidebar.getByText('发布', { exact: true }).click();
  await sidebar.getByTestId('publish-slug-input').fill('my-pmtiles-default-zoom');
  await sidebar.getByText('确认发布').click();

  const publicContext = await context.browser().newContext();
  const docsPage = await publicContext.newPage();
  await docsPage.goto(`${workerServer.url}/tiles/my-pmtiles-default-zoom/docs`);
  await expect(docsPage.locator('pre code')).toContainText('const publishedMinZoom = null;');
  await expect(docsPage.locator('pre code')).toContainText('const publishedMaxZoom = null;');
  await expect(docsPage.locator('pre code')).toContainText(
    'let resolvedMinZoom = publishedMinZoomClamped ?? headerMinZoom;',
  );
  await expect(docsPage.locator('pre code')).toContainText(
    'let resolvedMaxZoom = publishedMaxZoomClamped ?? headerMaxZoom;',
  );
  const configTable = docsPage.locator('table').first();
  await expect(configTable.locator('tr', { hasText: 'Zoom Range' })).toContainText(
    `${pmtilesHeader.minZoom} - ${pmtilesHeader.maxZoom}`,
  );
  await expect(configTable.locator('tr', { hasText: 'Format' })).toContainText(
    tileTypeLabel(pmtilesHeader.tileType),
  );
  await publicContext.close();
});

test('PMTiles embed view clamps published zoom bounds to archive header', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  const pmtilesHeader = await readPmtilesHeaderSummary(pmtilesPath);
  const publishMinZoom = Math.min(22, pmtilesHeader.maxZoom + 5);
  const publishMaxZoom = Math.min(22, pmtilesHeader.maxZoom + 8);

  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  await page.getByTestId('file-input').setInputFiles(pmtilesPath);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === 'sample');
        return file?.status;
      },
      { message: 'wait for PMTiles upload to be ready', timeout: 10000 },
    )
    .toBe('ready');

  const filesResponse = await request.get('/api/files');
  expect(filesResponse.ok()).toBeTruthy();
  const files = await filesResponse.json();
  const file = files.find((item) => item.name === 'sample');
  expect(file).toBeDefined();

  const publishResponse = await request.post(`/api/files/${file.id}/publish`, {
    data: {
      slug: 'my-pmtiles-clamped-zoom',
      minZoom: publishMinZoom,
      maxZoom: publishMaxZoom,
    },
  });
  expect(publishResponse.ok()).toBeTruthy();

  const publicContext = await context.browser().newContext();
  const embedPage = await publicContext.newPage();
  await embedPage.goto(`${workerServer.url}/tiles/my-pmtiles-clamped-zoom/embed`);
  await expect(embedPage.getByTestId('tile-embed-page')).toBeVisible();

  await expect
    .poll(
      async () => {
        return embedPage.evaluate(() => {
          const map = window.__mapflowPublicTileMap;
          const view = map?.getView?.();
          return view ? { minZoom: view.getMinZoom(), maxZoom: view.getMaxZoom() } : null;
        });
      },
      { message: 'wait for PMTiles embed zoom bounds to initialize', timeout: 10000 },
    )
    .toMatchObject({
      minZoom: pmtilesHeader.maxZoom,
      maxZoom: pmtilesHeader.maxZoom,
    });

  const docsPage = await publicContext.newPage();
  await docsPage.goto(`${workerServer.url}/tiles/my-pmtiles-clamped-zoom/docs`);
  const configTable = docsPage.locator('table').first();
  await expect(configTable.locator('tr', { hasText: 'Zoom Range' })).toContainText(
    `${pmtilesHeader.maxZoom} - ${pmtilesHeader.maxZoom}`,
  );
  await expect(docsPage.locator('pre code')).toContainText('const publishedMinZoomClamped =');
  await expect(docsPage.locator('pre code')).toContainText(
    'if (resolvedMinZoom > resolvedMaxZoom)',
  );

  await publicContext.close();
});

test('publish PMTiles with one zoom edit keeps the other bound unset', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  await page.getByTestId('file-input').setInputFiles(pmtilesPath);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === 'sample');
        return file?.status;
      },
      { message: 'wait for PMTiles upload to be ready', timeout: 10000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  let publishPayload = null;
  await page.route('**/api/files/*/publish', async (route) => {
    publishPayload = route.request().postDataJSON();
    await route.continue();
  });
  await sidebar.getByText('Publish', { exact: true }).click();
  await sidebar.getByText('发布', { exact: true }).click();
  await sidebar.getByTestId('publish-slug-input').fill('my-pmtiles-min-only');
  const publishZoomInputs = sidebar.locator('input[type="number"]');
  await publishZoomInputs.nth(0).fill('1');
  await sidebar.getByText('确认发布').click();

  await expect(sidebar.getByText('已发布')).toBeVisible();
  expect(publishPayload).toBeTruthy();
  expect(publishPayload.minZoom).toBe(1);
  expect(Object.prototype.hasOwnProperty.call(publishPayload, 'maxZoom')).toBeFalsy();
  await page.unroute('**/api/files/*/publish');

  const publicContext = await context.browser().newContext();
  const metaResponse = await publicContext.request.get(
    `${workerServer.url}/tiles/my-pmtiles-min-only/meta`,
  );
  expect(metaResponse.ok()).toBeTruthy();
  const metaJson = await metaResponse.json();
  expect(metaJson.minZoom).toBe(1);

  const docsPage = await publicContext.newPage();
  await docsPage.goto(`${workerServer.url}/tiles/my-pmtiles-min-only/docs`);
  await expect(docsPage.locator('pre code')).toContainText('const publishedMinZoom = 1;');
  await publicContext.close();
});

test('publish PMTiles with only max zoom edit keeps min bound unset', async ({
  page,
  context,
  request,
  workerServer,
}) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  await page.getByTestId('file-input').setInputFiles(pmtilesPath);

  await expect
    .poll(
      async () => {
        const response = await request.get('/api/files');
        if (!response.ok()) return null;
        const files = await response.json();
        const file = files.find((item) => item.name === 'sample');
        return file?.status;
      },
      { message: 'wait for PMTiles upload to be ready', timeout: 10000 },
    )
    .toBe('ready');

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  let publishPayload = null;
  await page.route('**/api/files/*/publish', async (route) => {
    publishPayload = route.request().postDataJSON();
    await route.continue();
  });
  await sidebar.getByText('Publish', { exact: true }).click();
  await sidebar.getByText('发布', { exact: true }).click();
  await sidebar.getByTestId('publish-slug-input').fill('my-pmtiles-max-only');
  const publishZoomInputs = sidebar.locator('input[type="number"]');
  await publishZoomInputs.nth(1).fill('5');
  await sidebar.getByText('确认发布').click();

  await expect(sidebar.getByText('已发布')).toBeVisible();
  expect(publishPayload).toBeTruthy();
  expect(publishPayload.maxZoom).toBe(5);
  expect(Object.prototype.hasOwnProperty.call(publishPayload, 'minZoom')).toBeFalsy();
  await page.unroute('**/api/files/*/publish');

  const publicContext = await context.browser().newContext();
  const metaResponse = await publicContext.request.get(
    `${workerServer.url}/tiles/my-pmtiles-max-only/meta`,
  );
  expect(metaResponse.ok()).toBeTruthy();
  const metaJson = await metaResponse.json();
  expect(metaJson.maxZoom).toBe(5);

  const docsPage = await publicContext.newPage();
  await docsPage.goto(`${workerServer.url}/tiles/my-pmtiles-max-only/docs`);
  await expect(docsPage.locator('pre code')).toContainText('const publishedMaxZoom = 5;');
  await publicContext.close();
});

test('publish with default slug (empty input)', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();

  // Switch to Publish tab
  await sidebar.getByText('Publish', { exact: true }).click();

  const publishButton = sidebar.getByText('发布', { exact: true });
  await publishButton.click();

  const confirmButton = sidebar.getByText('确认发布');
  await confirmButton.click();

  await expect(sidebar.getByText('已发布')).toBeVisible();
  await expect(sidebar.getByText('复制地址')).toBeVisible();
});

test('slug validation: invalid characters', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();

  // Switch to Publish tab
  await sidebar.getByText('Publish', { exact: true }).click();

  const publishButton = sidebar.getByText('发布', { exact: true });
  await publishButton.click();

  const slugInput = sidebar.getByTestId('publish-slug-input');
  await slugInput.fill('invalid slug!');

  await expect(
    sidebar.locator('.alert', { hasText: '仅支持字母、数字、连字符和下划线' }),
  ).toBeVisible();

  const confirmButton = sidebar.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});

test('slug validation: too long', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();

  // Switch to Publish tab
  await sidebar.getByText('Publish', { exact: true }).click();

  const publishButton = sidebar.getByText('发布', { exact: true });
  await publishButton.click();

  const slugInput = sidebar.getByTestId('publish-slug-input');
  const longSlug = 'a'.repeat(101);
  await slugInput.fill(longSlug);

  await expect(sidebar.getByText('URL 标识不能超过 100 个字符')).toBeVisible();

  const confirmButton = sidebar.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});
