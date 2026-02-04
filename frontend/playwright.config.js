import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  expect: {
    timeout: 10_000
  },
  use: {
    baseURL: `http://localhost:${process.env.TEST_PORT || 3000}`,
    trace: 'on-first-retry'
  },
  webServer: {
    command: 'cargo run --manifest-path backend/Cargo.toml',
    cwd: '..',
    port: Number(process.env.TEST_PORT || 3000),
    reuseExistingServer: !process.env.CI,
    env: {
      WEB_DIST: 'frontend/dist',
      UPLOAD_DIR: 'tmp/test-uploads',
      PORT: String(process.env.TEST_PORT || 3000)
    }
  }
});
