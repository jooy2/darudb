/**
 * The end-to-end tests: one scenario, run against both hosts of the same
 * screens. The `web` project drives Chromium against `server/main.ts`, which
 * Playwright starts on its own port with a database folder of its own, and
 * the `electron` project launches the app itself. `npm test` builds the
 * page first, since both load `dist/`.
 */
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

const PORT = 4317;

// Playwright loads this file again in each worker, which inherits the
// runner's environment, so the folder is made once.
process.env.DARUDB_SAMPLE_WEB_DIR ??= mkdtempSync(join(tmpdir(), 'darudb-sample-web-'));

export default defineConfig({
  testDir: 'tests',
  // The scenario's steps follow each other on one database.
  workers: 1,
  fullyParallel: false,
  timeout: 120_000,
  expect: { timeout: 30_000 },
  forbidOnly: process.env.CI !== undefined,
  reporter: process.env.CI === undefined ? 'list' : [['list'], ['html', { open: 'never' }]],
  globalTeardown: './tests/teardown.ts',
  use: {
    trace: 'retain-on-failure',
    actionTimeout: 15_000,
    viewport: { width: 1360, height: 860 }
  },
  projects: [
    {
      name: 'web',
      use: { browserName: 'chromium', baseURL: `http://127.0.0.1:${PORT}` }
    },
    {
      name: 'electron'
    }
  ],
  webServer: {
    command: 'node server/main.ts',
    url: `http://127.0.0.1:${PORT}`,
    env: { PORT: String(PORT), DARUDB_SAMPLE_DIR: process.env.DARUDB_SAMPLE_WEB_DIR },
    reuseExistingServer: false
  }
});
