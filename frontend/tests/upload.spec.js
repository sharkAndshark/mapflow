import { test, expect } from '@playwright/test';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');
const uploadDir = path.resolve(__dirname, '../../tmp/test-uploads');

const geojsonPath = path.join(fixturesDir, 'sample.geojson');
const shapefileZip = path.join(fixturesDir, 'roads.zip');

test.beforeEach(() => {
  fs.rmSync(uploadDir, { recursive: true, force: true });
  fs.mkdirSync(uploadDir, { recursive: true });
  fs.writeFileSync(path.join(uploadDir, 'index.json'), '[]');
});

async function uploadFile(page, filePath) {
  const input = page.getByTestId('file-input');
  await input.setInputFiles(filePath);
}

test('initial load shows existing uploads', async ({ page }) => {
  const seeded = [
    {
      id: 'seed-1',
      name: 'existing',
      type: 'geojson',
      size: 42,
      uploadedAt: new Date('2026-02-04T10:00:00Z').toISOString(),
      status: 'uploaded',
      crs: null,
      path: './uploads/seed-1/existing.geojson'
    }
  ];
  fs.writeFileSync(path.join(uploadDir, 'index.json'), JSON.stringify(seeded, null, 2));

  await page.goto('/');

  const row = page.getByRole('button', { name: /existing/ });
  await expect(row.getByText('existing', { exact: true })).toBeVisible();
  await expect(row.getByText('geojson', { exact: true })).toBeVisible();
  await expect(row.getByText('已上传', { exact: true })).toBeVisible();
});

test('upload geojson and show in list', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('empty-state')).toBeVisible();

  await uploadFile(page, geojsonPath);

  const row = page.getByRole('button', { name: /sample/ });
  await expect(row.getByText('sample', { exact: true })).toBeVisible();
  await expect(row.getByText('geojson', { exact: true })).toBeVisible();
  await expect(row.getByText('已上传', { exact: true })).toBeVisible();
});

test('upload shapefile zip and show in list', async ({ page }) => {
  await page.goto('/');

  await uploadFile(page, shapefileZip);

  await expect(page.getByText('roads')).toBeVisible();
  await expect(page.getByText('shapefile')).toBeVisible();
  await expect(page.getByText('已上传')).toBeVisible();
});
