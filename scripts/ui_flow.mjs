import { chromium } from 'playwright';
import fs from 'fs';
import path from 'path';

const BASE = 'http://127.0.0.1:3000';
const OUTPUT_DIR = '/Users/zhangyijun/RiderProjects/mapflow/output/playwright';
const ZIP_PATH = '/Users/zhangyijun/RiderProjects/mapflow/tests/fixtures/gis_osm_buildings_a_free_1.zip';

function log(step) {
  console.log(`STEP: ${step}`);
}

function lonLatToTile(lon, lat, z) {
  const maxLat = 85.0511;
  const clampedLat = Math.max(Math.min(lat, maxLat), -maxLat);
  const latRad = (clampedLat * Math.PI) / 180;
  const n = 2 ** z;
  const x = Math.floor(((lon + 180) / 360) * n);
  const y = Math.floor((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2 * n);
  return { x, y };
}

async function run() {
  fs.mkdirSync(OUTPUT_DIR, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });

  log('Open app');
  await page.goto(BASE, { waitUntil: 'networkidle' });

  log('Upload shapefile');
  await page.setInputFiles('input[type="file"]', ZIP_PATH);

  await page.waitForTimeout(2000);
  await page.waitForSelector('.node-card--resource', { timeout: 20000 });

  const resourceName = await page.textContent('.node-card--resource .node-title');
  log(`Resource created: ${resourceName}`);

  log('Create Layer node');
  await page.click('.react-flow__pane', { button: 'right' });
  await page.click('text=Add Layer Node');
  await page.waitForSelector('.node-card--layer', { timeout: 10000 });

  log('Create XYZ node');
  await page.click('.react-flow__pane', { button: 'right' });
  await page.click('text=Add XYZ Node');
  await page.waitForSelector('.node-card--xyz', { timeout: 10000 });

  log('Open Layer inspector');
  await page.dblclick('.node-card--layer');
  await page.waitForSelector('.inspector', { timeout: 5000 });

  log('Select resource for layer');
  await page.selectOption('select', { label: resourceName?.trim() || '' });
  await page.click('text=Load Fields');
  await page.waitForTimeout(1000);

  const fieldCheckboxes = await page.$$('.field-grid input[type="checkbox"]');
  if (fieldCheckboxes.length > 0) {
    await fieldCheckboxes[0].check();
    if (fieldCheckboxes.length > 1) {
      await fieldCheckboxes[1].check();
    }
  }

  log('Open XYZ inspector');
  await page.dblclick('.node-card--xyz');
  await page.waitForSelector('.inspector', { timeout: 5000 });

  log('Select layer for XYZ');
  const layerLabel = await page.textContent('.node-card--layer .node-title');
  if (layerLabel) {
    await page.check(`.field-grid label:has-text("${layerLabel.trim()}") input`);
  }

  await page.click('text=Use Resource Bounds');
  await page.click('text=Use Resource Center');

  log('Validate config');
  await page.click('text=Validate');
  await page.waitForTimeout(2000);

  log('Apply config');
  await page.click('text=Apply');
  await page.waitForTimeout(2000);

  await page.screenshot({ path: path.join(OUTPUT_DIR, 'flow.png'), fullPage: true });

  log('Request tile via API');
  const configResp = await page.request.get(`${BASE}/config`);
  const config = await configResp.json();
  const xyz = (config.nodes || []).find((n) => n.type === 'xyz');
  if (!xyz) throw new Error('No xyz node found after apply');

  const [lon, lat, z] = xyz.center;
  const { x, y } = lonLatToTile(lon, lat, Math.round(z));
  const tileResp = await page.request.get(`${BASE}/tiles/${Math.round(z)}/${x}/${y}.pbf`);

  console.log('TILE_STATUS', tileResp.status());

  await browser.close();
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
