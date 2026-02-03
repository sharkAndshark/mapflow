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

  const resetResp = await fetch(`${BASE}/config`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ version: '0.1.0', nodes: [], edges: [] }),
  });
  if (!resetResp.ok) {
    console.log('RESET_FAILED', resetResp.status);
  }

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  page.setDefaultTimeout(60000);
  page.on('console', (msg) => console.log('BROWSER_LOG', msg.text()));

  log('Open app');
  await page.goto(BASE, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => {
    const el = document.querySelector('.react-flow');
    return el && el.clientWidth > 200 && el.clientHeight > 200;
  });

  log('Upload shapefile');
  await page.fill('input[placeholder="4326"]', '4326');
  await page.setInputFiles('input[type="file"]', ZIP_PATH);

  await page.waitForTimeout(2000);
  await page.waitForSelector('.node-card--resource', { timeout: 20000 });

  const resourceName = await page.textContent('.node-card--resource .node-title');
  const resourceNodeId = await page.evaluate(() => {
    const card = document.querySelector('.node-card--resource');
    return card?.closest('.react-flow__node')?.getAttribute('data-id') || '';
  });
  log(`Resource created: ${resourceName}`);

  log('Create Layer node');
  await page.dispatchEvent('.react-flow__pane', 'contextmenu', { clientX: 900, clientY: 260 });
  await page.waitForSelector('.context-menu__item', { timeout: 5000 });
  await page.click('.context-menu__item:has-text("Add Layer Node")');
  await page.waitForSelector('.node-card--layer', { timeout: 10000 });
  const layerNodeId = await page.evaluate(() => {
    const cards = Array.from(document.querySelectorAll('.node-card--layer'));
    const card = cards[cards.length - 1];
    return card?.closest('.react-flow__node')?.getAttribute('data-id') || '';
  });

  log('Create XYZ node');
  await page.dispatchEvent('.react-flow__pane', 'contextmenu', { clientX: 1050, clientY: 520 });
  await page.waitForSelector('.context-menu__item', { timeout: 5000 });
  await page.click('.context-menu__item:has-text("Add XYZ Node")');
  await page.waitForSelector('.node-card--xyz', { timeout: 10000 });
  const xyzNodeId = await page.evaluate(() => {
    const cards = Array.from(document.querySelectorAll('.node-card--xyz'));
    const card = cards[cards.length - 1];
    return card?.closest('.react-flow__node')?.getAttribute('data-id') || '';
  });

  log('Open Layer inspector');
  await page.evaluate((id) => {
    const node = document.querySelector(`.react-flow__node[data-id="${id}"]`);
    if (node) {
      node.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    }
  }, layerNodeId);
  await page.waitForSelector('.inspector', { timeout: 5000 });

  log('Select resource for layer');
  if (resourceNodeId) {
    await page.selectOption('select', { value: resourceNodeId });
  } else {
    await page.selectOption('select', { index: 1 });
  }
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
  await page.evaluate((id) => {
    const node = document.querySelector(`.react-flow__node[data-id="${id}"]`);
    if (node) {
      node.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    }
  }, xyzNodeId);
  await page.waitForSelector('.inspector', { timeout: 5000 });

  log('Select layer for XYZ');
  await page.check('.inspector .field-grid input[type="checkbox"]');

  await page.click('text=Use Resource Bounds');
  await page.click('text=Use Resource Center');

  await page.waitForSelector(`.react-flow__node[data-id="${layerNodeId}"] .node-status--ok`, { timeout: 5000 });
  await page.waitForSelector(`.react-flow__node[data-id="${xyzNodeId}"] .node-status--ok`, { timeout: 5000 });

  log('Validate config');
  await page.click('text=Validate');
  await page.waitForTimeout(2000);

  const validationError = await page.$('.panel-alert--error');
  if (validationError) {
    const errorText = await validationError.textContent();
    console.log('VALIDATION_ERRORS', errorText?.trim());
  } else {
    log('Apply config');
    await page.click('text=Apply');
    await page.waitForTimeout(2000);
  }

  await page.screenshot({ path: path.join(OUTPUT_DIR, 'flow.png'), fullPage: true });

  log('Request tile via API');
  const configController = new AbortController();
  const configTimeout = setTimeout(() => configController.abort(), 15000);
  const configResp = await fetch(`${BASE}/config`, { signal: configController.signal });
  clearTimeout(configTimeout);
  const config = await configResp.json();
  const xyz = (config.nodes || []).find((n) => n.type === 'xyz');
  if (!xyz) throw new Error('No xyz node found after apply');

  const [lon, lat, z] = xyz.center;
  const { x, y } = lonLatToTile(lon, lat, Math.round(z));
  const tileController = new AbortController();
  const tileTimeout = setTimeout(() => tileController.abort(), 30000);
  try {
    const tileResp = await fetch(`${BASE}/tiles/${Math.round(z)}/${x}/${y}.pbf`, { signal: tileController.signal });
    console.log('TILE_STATUS', tileResp.status);
  } catch (err) {
    console.log('TILE_ERROR', err?.name || err);
  } finally {
    clearTimeout(tileTimeout);
  }

  await browser.close();
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
