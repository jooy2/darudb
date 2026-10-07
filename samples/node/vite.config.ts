/**
 * Builds the screens, `ui/`, into `dist/`, which the Electron app loads
 * from disk and the web server serves. `base: './'` keeps every path in the
 * page relative, since Electron loads it from a `file:` URL.
 *
 * `npm run dev` serves the page with hot reloading, and runs the web
 * sample's API in the same process, so the page has a database to talk to.
 * The scripts load this file with `--configLoader native`, so that Node.js
 * imports it and the sample's own TypeScript as they are, rather than Vite
 * bundling them with the engine's native addon.
 */
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import type { Plugin } from 'vite';

const ROOT = fileURLToPath(new URL('.', import.meta.url));

/** What the built page may load: only its own files, and styles set by its scripts. */
const CONTENT_SECURITY_POLICY = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self' data:",
  "connect-src 'self'"
].join('; ');

/** The policy, in the built page only: hot reloading runs scripts inline. */
const contentSecurityPolicy = (): Plugin => ({
  name: 'darudb-sample-csp',
  apply: 'build',
  transformIndexHtml: () => [
    {
      tag: 'meta',
      attrs: { 'http-equiv': 'Content-Security-Policy', content: CONTENT_SECURITY_POLICY },
      injectTo: 'head-prepend'
    }
  ]
});

/** The web sample's API inside the development server. */
const sampleApi = (): Plugin => ({
  name: 'darudb-sample-api',
  apply: 'serve',
  async configureServer(server) {
    const { SampleStore } = await import('./core/store.ts');
    const { handleApi } = await import('./server/api.ts');
    const store = await SampleStore.open(
      process.env.DARUDB_SAMPLE_DIR ?? join(ROOT, '.data'),
      'web'
    );

    server.middlewares.use((request, response, next) => {
      handleApi(store, request, response).then((handled) => {
        if (!handled) {
          next();
        }
      }, next);
    });
    server.httpServer?.once('close', () => {
      void store.close();
    });
  }
});

export default defineConfig({
  base: './',
  plugins: [react(), contentSecurityPolicy(), sampleApi()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 2048
  },
  server: {
    host: '127.0.0.1'
  }
});
