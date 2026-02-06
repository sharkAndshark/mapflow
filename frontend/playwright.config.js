import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.js',
  testIgnore: ['**/unit/**'],
  timeout: 60_000,
  expect: {
    timeout: 10_000,
  },
  use: {
    // We don't set a global baseURL because each worker has its own server.
    // Our custom fixture handles URL resolution.
    trace: 'on-first-retry',
  },
  // Removed webServer config because we spawn servers per-worker in fixtures.js
});
