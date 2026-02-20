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

  // Click the row to select it and show detail sidebar
  await row.click();

  const sidebar = page.locator('.detail-sidebar');
  await expect(sidebar).toBeVisible();

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

  const publishButton = sidebar.getByText('发布', { exact: true });
  await publishButton.click();

  const slugInput = sidebar.getByTestId('publish-slug-input');
  const longSlug = 'a'.repeat(101);
  await slugInput.fill(longSlug);

  await expect(sidebar.getByText('URL 标识不能超过 100 个字符')).toBeVisible();

  const confirmButton = sidebar.getByText('确认发布');
  await expect(confirmButton).toBeDisabled();
});
