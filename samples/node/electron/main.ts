/**
 * The Electron sample's main process, which holds the database: a renderer
 * is sandboxed and cannot load a native addon, so the window asks for
 * everything over IPC, through the bridge `preload.cjs` gives it, and
 * `dispatch.ts` checks what arrives.
 *
 * The database lives in a `data` folder under the app's user data folder,
 * or in `DARUDB_SAMPLE_DIR` when that is set, which is how the end-to-end
 * tests give each run a folder of its own. `DARUDB_SAMPLE_UI_URL` loads the
 * page from Vite's development server instead of `dist/`, for working on
 * the screens with hot reloading. `DARUDB_SAMPLE_HIDDEN=1` keeps the window
 * off the screen and the app out of the Dock, which the end-to-end tests
 * set, so that a run does not take the screen from whoever started it; they
 * drive the page all the same.
 *
 * Electron runs this TypeScript as it is, since its Node.js strips the types
 * of a `.ts` file it loads.
 */
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { app, BrowserWindow, ipcMain } from 'electron';

import { dispatch } from '../core/dispatch.ts';
import { SampleStore } from '../core/store.ts';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

let store: SampleStore | null = null;
let closing = false;

const hidden = process.env.DARUDB_SAMPLE_HIDDEN === '1';

const openWindow = async (): Promise<void> => {
  const window = new BrowserWindow({
    width: 1360,
    height: 860,
    minWidth: 960,
    minHeight: 600,
    title: 'DaruDB Sample',
    show: !hidden,
    webPreferences: {
      preload: join(ROOT, 'electron', 'preload.cjs'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      // A hidden window's timers and frames would be slowed down as a
      // window in the background's are, and the tests wait on them.
      backgroundThrottling: !hidden
    }
  });
  const url = process.env.DARUDB_SAMPLE_UI_URL;

  // The window shows the sample's own page and nothing else: no new windows,
  // and no navigation away from it.
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event) => {
    event.preventDefault();
  });

  if (url === undefined) {
    await window.loadFile(join(ROOT, 'dist', 'index.html'));
  } else {
    await window.loadURL(url);
  }
};

app.whenReady().then(async () => {
  if (hidden) {
    app.dock?.hide();
  }

  const directory = process.env.DARUDB_SAMPLE_DIR ?? join(app.getPath('userData'), 'data');

  store = await SampleStore.open(directory, 'electron');

  const opened = store;

  ipcMain.handle('sample:call', (event, method: unknown, args: unknown) =>
    dispatch(opened, method, args, (progress) => {
      if (!event.sender.isDestroyed()) {
        event.sender.send('sample:progress', progress);
      }
    })
  );

  await openWindow();
});

// The database closes before the app quits, so a quit never cuts a write
// short. Quitting waits for the close once, then quits for real.
app.on('before-quit', (event) => {
  if (store === null || closing) {
    return;
  }

  event.preventDefault();
  closing = true;
  store
    .close()
    .catch((error: unknown) => {
      console.error(error);
    })
    .finally(() => app.quit());
});

app.on('window-all-closed', () => {
  app.quit();
});
