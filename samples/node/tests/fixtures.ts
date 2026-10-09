/**
 * What the scenario runs against: a page of the sample, and a way to start
 * the app again on the same database.
 *
 * In the `electron` project that is the app's first window, launched with a
 * database folder of its own, and starting again quits the app and launches
 * it once more on that folder, which is how the scenario sees that what was
 * committed is still in the file. In the `web` project it is a page of the
 * server Playwright started, which is reset first, and starting again loads
 * the page again from the same server.
 *
 * The Electron window stays hidden, and the app out of the Dock, unless
 * `DARUDB_SAMPLE_SHOW=1` asks to watch it.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { _electron as electron, test as base } from '@playwright/test';
import type { ElectronApplication, Page } from '@playwright/test';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

export interface Sample {
  readonly page: Page;
  /** Starts the app again on the same database, and gives its new page. */
  restart(): Promise<Page>;
}

const shown = process.env.DARUDB_SAMPLE_SHOW === '1';

const launch = async (directory: string): Promise<ElectronApplication> => {
  const app = await electron.launch({
    args: [ROOT],
    cwd: ROOT,
    env: { ...process.env, DARUDB_SAMPLE_DIR: directory, DARUDB_SAMPLE_HIDDEN: shown ? '0' : '1' }
  });

  await app.firstWindow();

  const visible = await app.evaluate(({ BrowserWindow }) =>
    BrowserWindow.getAllWindows().some((window) => window.isVisible())
  );

  if (visible !== shown) {
    throw new Error(`the app's window is ${visible ? 'on' : 'off'} the screen`);
  }

  return app;
};

export const test = base.extend<object, { sample: Sample }>({
  sample: [
    async ({ browser }, use, workerInfo) => {
      if (workerInfo.project.name === 'electron') {
        const directory = mkdtempSync(join(tmpdir(), 'darudb-sample-electron-'));
        let app = await launch(directory);
        let page = await app.firstWindow();

        await page.setViewportSize({ width: 1360, height: 860 });
        await use({
          get page() {
            return page;
          },
          restart: async () => {
            await app.close();
            app = await launch(directory);
            page = await app.firstWindow();
            await page.setViewportSize({ width: 1360, height: 860 });

            return page;
          }
        });
        await app.close();
        rmSync(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });

        return;
      }

      const baseURL = workerInfo.project.use.baseURL;
      const context = await browser.newContext({ baseURL, viewport: { width: 1360, height: 860 } });
      const page = await context.newPage();

      await page.request.post('api/reset', { data: {} });
      await page.goto('/');
      await use({
        page,
        restart: async () => {
          await page.reload();

          return page;
        }
      });
      await context.close();
    },
    { scope: 'worker' }
  ]
});

export { expect } from '@playwright/test';
