import { test, expect } from './fixtures';
import path from 'path';
import { fileURLToPath } from 'url';
import { loginUser, setupTestUser } from './auth-helper.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const geojsonPath = path.join(fixturesDir, 'sample.geojson');

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

  const publishButton = row.getByText('发布');
  await expect(publishButton).toBeVisible();
  await publishButton.click();

  const modal = page.locator('.modal-content');
  await expect(modal).toBeVisible();
  await expect(modal.getByText('发布文件')).toBeVisible();
  await expect(modal.getByText('sample')).toBeVisible();

  const slugInput = modal.getByLabel('URL 标识（可选）');
  await slugInput.fill('my-custom-map');

  const confirmButton = modal.getByText('确认发布');
  await expect(confirmButton).toBeEnabled();
  await confirmButton.click();

  await expect(modal).toBeHidden();

  await expect(row.getByText('复制')).toBeVisible();
  await expect(row.getByText('取消发布')).toBeVisible();
  await expect(row.getByRole('button', { name: /^发布$/ })).toHaveCount(0);

  const copyButton = row.getByText('复制');
  await copyButton.click();

  await page.waitForTimeout(1000);

  const publicContext = await context.browser().newContext();
  const publicRequest = publicContext.request;
  const response = await publicRequest.get(`${workerServer.url}/tiles/my-custom-map/0/0/0`);
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toContain('application/vnd.mapbox-vector-tile');
  expect(response.headers()['cache-control']).toContain('public, max-age=300');
  await publicContext.close();

  await page.goto('/');
  await expect(page.locator('.page')).toBeVisible();

  const readyRow = page
    .locator('.row', { hasText: 'sample' })
    .filter({ hasText: '已就绪' })
    .first();
  await expect(readyRow).toBeVisible();

  page.once('dialog', (dialog) => dialog.accept());
  const unpublishButton = readyRow.getByText('取消发布');
  await expect(unpublishButton).toBeVisible();
  await unpublishButton.click();

  await expect(readyRow.getByText('发布')).toBeVisible();
  await expect(readyRow.getByText('取消发布')).not.toBeVisible();

  const anonContext = await context.browser().newContext();
  const errorResponse = await anonContext.request.get(
    `${workerServer.url}/tiles/my-custom-map/0/0/0`,
  );
  expect(errorResponse.status()).toBe(404);
  await anonContext.close();
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

  const modal = page.locator('.modal-content');
  await expect(modal).toBeVisible();

  const confirmButton = modal.getByText('确认发布');
  await confirmButton.click();

  await expect(modal).toBeHidden();
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

  const modal = page.locator('.modal-content');
  await expect(modal).toBeVisible();

  const slugInput = modal.getByLabel('URL 标识（可选）');
  await slugInput.fill('invalid slug!');

  await expect(
    modal.locator('.alert', { hasText: '仅支持字母、数字、连字符和下划线' }),
  ).toBeVisible();

  const confirmButton = modal.getByText('确认发布');
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

  const modal = page.locator('.modal-content');
  await expect(modal).toBeVisible();

  const slugInput = modal.getByLabel('URL 标识（可选）');
  const longSlug = 'a'.repeat(101);
  await slugInput.fill(longSlug);

  await expect(modal.getByText('URL 标识不能超过 100 个字符')).toBeVisible();

  const confirmButton = modal.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});
