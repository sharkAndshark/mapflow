import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const geojsonPath = path.join(fixturesDir, 'sample.geojson');

test.beforeEach(async ({ workerServer, request }) => {
  await workerServer.reset();
  await setupTestUser(request);
});

test('publish flow: upload file, publish with custom slug, access public tiles', async ({
  page,
  context,
  request,
}) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();

  const publishButton = row.getByText('发布');
  await expect(publishButton).toBeVisible();
  await publishButton.click();

  await expect(page.getByText('发布文件')).toBeVisible();
  await expect(page.getByText('sample')).toBeVisible();

  const slugInput = page.getByPlaceholder('sample');
  await slugInput.fill('my-custom-map');

  const confirmButton = page.getByText('确认发布');
  await expect(confirmButton).toBeEnabled();
  await confirmButton.click();

  await expect(page.getByText('发布文件')).not().toBeVisible();

  await expect(row.getByText('复制')).toBeVisible();
  await expect(row.getByText('取消发布')).toBeVisible();
  await expect(row.getByText('发布')).not().toBeVisible();

  const copyButton = row.getByText('复制');
  await copyButton.click();

  await page.waitForTimeout(1000);

  const newPage = await context.newPage();
  await newPage.goto('/tiles/my-custom-map/0/0/0');
  const response = await newPage.waitForResponse(
    (response) => response.url().includes('/tiles/my-custom-map/0/0/0') && response.ok(),
  );
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toContain('application/vnd.mapbox-vector-tile');
  expect(response.headers()['cache-control']).toContain('public, max-age=300');
  await newPage.close();

  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();

  const readyRow = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: '已就绪' })
    .first();
  await expect(readyRow).toBeVisible();

  const unpublishButton = readyRow.getByText('取消发布');
  await expect(unpublishButton).toBeVisible();
  await unpublishButton.click();

  await expect(readyRow.getByText('发布')).toBeVisible();
  await expect(readyRow.getByText('取消发布')).not().toBeVisible();

  const unpublishedPage = await context.newPage();
  await unpublishedPage.goto('/tiles/my-custom-map/0/0/0');
  const errorResponse = await unpublishedPage.waitForResponse(
    (response) =>
      response.url().includes('/tiles/my-custom-map/0/0/0') && response.status() === 404,
  );
  expect(errorResponse.status()).toBe(404);
  await unpublishedPage.close();
});

test('publish with default slug (empty input)', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();

  const publishButton = row.getByText('发布');
  await publishButton.click();

  const confirmButton = page.getByText('确认发布');
  await confirmButton.click();

  await expect(page.getByText('发布文件')).not().toBeVisible();
  await expect(row.getByText('复制')).toBeVisible();
});

test('slug validation: invalid characters', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();

  const publishButton = row.getByText('发布');
  await publishButton.click();

  const slugInput = page.getByPlaceholder('sample');
  await slugInput.fill('invalid slug!');

  await expect(page.getByText('仅支持字母、数字、连字符和下划线')).toBeVisible();

  const confirmButton = page.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});

test('slug validation: too long', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();
  const input = page.getByTestId('file-input');
  await input.setInputFiles(geojsonPath);

  const row = page.locator('.row', { hasText: 'sample' }).filter({ hasText: '已就绪' }).first();
  await expect(row).toBeVisible();

  const publishButton = row.getByText('发布');
  await publishButton.click();

  const slugInput = page.getByPlaceholder('sample');
  const longSlug = 'a'.repeat(101);
  await slugInput.fill(longSlug);

  await expect(page.getByText('URL 标识不能超过 100 个字符')).toBeVisible();

  const confirmButton = page.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});
