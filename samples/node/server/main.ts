/**
 * The web sample: a server on this machine that holds the database and
 * serves the page `vite build` wrote into `dist/`. A browser cannot load the
 * engine, so the page asks this server for everything, through `api.ts`.
 *
 * `DARUDB_SAMPLE_DIR` names the folder the database file goes in, `.data/`
 * beside this sample by default, and `PORT` the port, 3000 by default. Run
 * it with `npm run web`, which builds the page first.
 */
import { readFile, stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { SampleStore } from '../core/store.ts';

import { handleApi } from './api.ts';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = join(ROOT, 'dist');
const HOST = '127.0.0.1';

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.json': 'application/json; charset=utf-8',
  '.woff2': 'font/woff2'
};

const directory = process.env.DARUDB_SAMPLE_DIR ?? join(ROOT, '.data');
const port = Number.parseInt(process.env.PORT ?? '3000', 10);

if (!Number.isInteger(port) || port < 0 || port > 65535) {
  throw new RangeError(`PORT must be a port number, not ${process.env.PORT}`);
}

/** The file in `dist/` a path names, or the page itself for any path that names none. */
const fileOf = async (pathname: string): Promise<string> => {
  const path = join(DIST, normalize(decodeURIComponent(pathname)));

  if (path.startsWith(DIST + sep)) {
    try {
      if ((await stat(path)).isFile()) {
        return path;
      }
    } catch {
      // Not there: the page answers, as it does for any path of its own.
    }
  }

  return join(DIST, 'index.html');
};

const serveFile = async (request: IncomingMessage, response: ServerResponse): Promise<void> => {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    response.writeHead(405, { allow: 'GET, HEAD' }).end();

    return;
  }

  const { pathname } = new URL(request.url ?? '/', 'http://localhost');
  const path = await fileOf(pathname);

  try {
    const body = await readFile(path);

    response.writeHead(200, {
      'content-type': CONTENT_TYPES[extname(path)] ?? 'application/octet-stream',
      'x-content-type-options': 'nosniff'
    });
    response.end(request.method === 'HEAD' ? undefined : body);
  } catch {
    response
      .writeHead(503, { 'content-type': 'text/plain; charset=utf-8' })
      .end(
        'The page is not built. Run `npm run build` in samples/node, or start with `npm run web`.'
      );
  }
};

const store = await SampleStore.open(directory, 'web');
const server = createServer((request, response) => {
  handleApi(store, request, response)
    .then((handled) => (handled ? undefined : serveFile(request, response)))
    .catch((error: unknown) => {
      console.error(error);

      if (!response.headersSent) {
        response.writeHead(500).end();
      }
    });
});

const onSignal = (): void => {
  server.close();
  store.close().then(
    () => process.exit(0),
    (error: unknown) => {
      console.error(error);
      process.exit(1);
    }
  );
};

process.once('SIGINT', onSignal);
process.once('SIGTERM', onSignal);

server.listen(port, HOST, () => {
  console.log(`DaruDB sample at http://${HOST}:${port}, keeping ${store.path}`);
});
