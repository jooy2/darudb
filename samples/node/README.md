# DaruDB sample for Node.js

One set of React screens with [Plass UI](https://plass.cdget.com), run as an Electron app and as a web page that a server on this machine serves. [The samples' README](../README.md) says what the app does, how the sample data is made, and what you need.

| Path        | What it is                                                                  |
| ----------- | --------------------------------------------------------------------------- |
| `core/`     | The schema, the sample data, the store, and what the screens send it        |
| `electron/` | The Electron main process, which holds the database, and the preload bridge |
| `server/`   | The local server for the web page, and its API                              |
| `ui/`       | The screens, which Vite builds into `dist/`                                 |
| `tests/`    | The end-to-end scenario, run in both hosts by Playwright                    |

Build `packages/node` first (`npm install` and `npm run build` there), then:

```bash
npm install
npm start
```

| Script              | What it does                                                         |
| ------------------- | -------------------------------------------------------------------- |
| `npm start`         | Builds the screens and opens the Electron app                        |
| `npm run web`       | Builds the screens and serves them at `http://127.0.0.1:3000`        |
| `npm run dev`       | Serves the screens with hot reloading, the API in the same process   |
| `npm test`          | Builds the screens and runs the scenario in Electron and in Chromium |
| `npm run typecheck` | Checks the types of everything here                                  |
| `npm run format`    | Checks the formatting                                                |

`DARUDB_SAMPLE_DIR` names the folder the database file goes in, and `PORT` the web server's port. `DARUDB_SAMPLE_UI_URL` makes the Electron app load the screens from Vite's development server, for working on them with hot reloading.
