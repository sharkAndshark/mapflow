// MapFlow E2E Test Fixtures
// This file orchestrates starting a dedicated backend server for each worker.
import { test as base } from '@playwright/test';
import { spawn } from 'child_process';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '../../');

// Helper to wait for port
const waitForPort = async (port, timeout = 30000) => {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    try {
      const res = await fetch(`http://localhost:${port}/api/test/is-initialized`);
      if (res.ok) return true;
    } catch (e) {
      // ignore
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Extend base test with our custom fixture
export const test = base.extend({
  // Override baseURL to point to our worker-specific server
  // Note: Playwright's `page` fixture uses this automatically if configured properly,
  // but since we change it per worker, we might need to set it on context/page.
  // Actually, setting `use: { baseURL }` in config is global.
  // Here we inject a worker-scoped server and provide its URL.

  workerServer: [
    async ({}, use, workerInfo) => {
      // 1. Setup paths and ports based on worker index
      // Worker index is 0-based. We'll use ports starting from 4000
      // to avoid conflict with default dev (3000) or other services.
      const workerId = workerInfo.workerIndex;
      const port = 4000 + workerId;
      const testRoot = path.resolve(__dirname, '../../tmp', `worker-${workerId}`);
      const dbPath = path.join(testRoot, 'mapflow.duckdb');
      const uploadDir = path.join(testRoot, 'uploads');

      // Ensure clean directory for this worker run
      // Note: We clean at startup of the worker, so tests within the worker reuse it
      // but we will implement per-test reset via API.
      fs.rmSync(testRoot, { recursive: true, force: true });
      fs.mkdirSync(uploadDir, { recursive: true });

      console.log(`[Worker ${workerId}] Starting backend on port ${port}...`);

      // 2. Spawn Backend Process
      // Prefer running the precompiled backend binary for speed.
      // Fallback to `cargo run` only when the binary is not available.
      const exeExt = process.platform === 'win32' ? '.exe' : '';
      const binaryCandidates = [
        path.join(repoRoot, 'target', 'debug', `backend${exeExt}`),
        path.join(repoRoot, 'backend', 'target', 'debug', `backend${exeExt}`),
      ];

      let backendCommand = 'cargo';
      let backendArgs = ['run', '--quiet', '--manifest-path', 'backend/Cargo.toml'];
      for (const candidate of binaryCandidates) {
        if (fs.existsSync(candidate)) {
          backendCommand = candidate;
          backendArgs = [];
          break;
        }
      }

      console.log(
        `[Worker ${workerId}] Backend cmd: ${backendCommand}${backendArgs.length ? ` ${backendArgs.join(' ')}` : ''}`,
      );

      const backendProcess = spawn(backendCommand, backendArgs, {
        cwd: repoRoot,
        env: {
          ...process.env,
          PORT: String(port),
          DB_PATH: dbPath,
          UPLOAD_DIR: uploadDir,
          MAPFLOW_TEST_MODE: '1', // Enable reset API
          // Front-end dist is optional for API tests, but if we want page.goto('/') to work
          // we need the backend to serve statics.
          // We can point to the real dist or a dummy one.
          WEB_DIST: path.resolve(__dirname, '../dist'),
        },
        stdio: 'pipe', // Capture output for debugging
      });

      // Pipe output for debug (optional, maybe noisy)
      backendProcess.stdout.on('data', (d) => console.log(`[Backend ${port}] ${d}`));
      backendProcess.stderr.on('data', (d) => console.error(`[Backend ${port}] ${d}`));

      // 3. Wait for readiness
      const ready = await waitForPort(port, 60000); // Increased timeout to 60s
      if (!ready) {
        throw new Error(`Backend failed to start on port ${port}`);
      }

      const serverUrl = `http://localhost:${port}`;

      // 4. Use
      await use({
        port,
        url: serverUrl,
        reset: async () => {
          // Helper to reset state via API
          const res = await fetch(`${serverUrl}/api/test/reset`, { method: 'POST' });
          if (!res.ok) {
            const body = await res.text().catch(() => '');
            throw new Error(
              `Reset failed: ${res.status} ${res.statusText}${body ? `: ${body}` : ''}`,
            );
          }
        },
        waitForFileReady: async (fileName, timeoutMs = 60000) => {
          const start = Date.now();
          let last = null;
          while (Date.now() - start < timeoutMs) {
            const res = await fetch(`${serverUrl}/api/files`);
            if (!res.ok) {
              await sleep(200);
              continue;
            }
            const files = await res.json();
            const f = files.find((x) => x.name === fileName);
            if (f) {
              last = f;
              if (f.status === 'ready') return f;
              if (f.status === 'failed') {
                throw new Error(`File processing failed: ${f.error || 'unknown error'}`);
              }
            }
            await sleep(250);
          }

          throw new Error(
            `Timeout waiting for file to be ready: ${fileName} (last=${last ? JSON.stringify(last) : 'null'})`,
          );
        },
      });

      // 5. Cleanup
      console.log(`[Worker ${workerId}] Stopping backend...`);
      backendProcess.kill('SIGTERM');

      // Wait for port to be released
      let retries = 50;
      while (retries > 0) {
        try {
          await fetch(`http://localhost:${port}/api/test/is-initialized`);
          await new Promise((r) => setTimeout(r, 100));
          retries--;
        } catch (e) {
          break;
        }
      }

      if (retries === 0) {
        console.log(`[Worker ${workerId}] Force killing backend...`);
        backendProcess.kill('SIGKILL');
      }
    },
    { scope: 'worker', auto: true },
  ], // auto: true means it starts for every worker automatically

  // Override page to use the worker server as base URL
  page: async ({ page, workerServer }, use) => {
    // Important: We must navigate relative to the specific worker server
    // But Playwright's page.goto('/') uses config.use.baseURL.
    // We can't easily change config.use.baseURL dynamically per worker in `test.extend`.
    // Instead, we can override `goto` or just set no baseURL in config and always use full URL.
    // OR: We create a derived fixture that wraps interactions.

    // Simplest: We just use `workerServer.url` in our tests instead of relying on baseURL
    // OR: We assume tests will use relative URLs and we intercept them? No.

    // Let's try to patch page.goto to prepend our server URL if relative
    // This mimics baseURL behavior but per-worker.
    const originalGoto = page.goto.bind(page);
    page.goto = async (url, options) => {
      if (url.startsWith('/')) {
        return originalGoto(`${workerServer.url}${url}`, options);
      }
      return originalGoto(url, options);
    };

    // Also patch request context if needed, but `page.request` is separate.
    // For `request` fixture, we also need to extend it.

    await use(page);
  },

  request: async ({ page, workerServer }, use) => {
    // Use page's request context to share cookies/session with the browser
    // But wrap it to support relative URLs
    const originalPost = page.request.post.bind(page.request);
    const originalGet = page.request.get.bind(page.request);
    const originalFetch = page.request.fetch.bind(page.request);

    const wrappedRequest = {
      ...page.request,
      post: async (url, options) => {
        const fullUrl = url.startsWith('/') ? `${workerServer.url}${url}` : url;
        return originalPost(fullUrl, options);
      },
      get: async (url, options) => {
        const fullUrl = url.startsWith('/') ? `${workerServer.url}${url}` : url;
        return originalGet(fullUrl, options);
      },
      fetch: async (url, options) => {
        const fullUrl = url.startsWith('/') ? `${workerServer.url}${url}` : url;
        return originalFetch(fullUrl, options);
      },
    };

    await use(wrappedRequest);
  },
});

export { expect } from '@playwright/test';
