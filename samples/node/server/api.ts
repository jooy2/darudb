/**
 * The HTTP side of the web sample: `POST /api/<method>` with the method's
 * arguments as JSON, answered with its `Reply` as JSON. `seed` answers with
 * one line of JSON per progress report and the reply on the last line, so
 * the page can draw its progress bar while the run goes on.
 *
 * The server writes a database for whoever asks, so it serves only this
 * machine (`main.ts` listens on the loopback address) and only pages of its
 * own origin: a request has to say it is JSON, which a page elsewhere cannot
 * send without asking first and being refused, and an `Origin` or a `Host`
 * header naming somewhere else is refused, which also keeps out a name that
 * resolves to this machine for a page somewhere else.
 *
 * Vite's development server runs the same handler, from `vite.config.ts`.
 */
import type { IncomingMessage, ServerResponse } from 'node:http';

import { dispatch } from '../core/dispatch.ts';
import type { Reply } from '../core/protocol.ts';
import type { SampleStore } from '../core/store.ts';

export const API_PREFIX = '/api/';

/** The largest request body accepted, in bytes. */
const BODY_LIMIT = 1 << 20;

const LOCAL_HOSTNAMES = ['localhost', '127.0.0.1', '[::1]'];

const failure = (code: string, message: string): Reply<never> => ({ ok: false, code, message });

const sendJson = (response: ServerResponse, status: number, body: unknown): void => {
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'x-content-type-options': 'nosniff'
  });
  response.end(JSON.stringify(body));
};

/** Whether the request comes from a page this server served, or from no page at all. */
const isOwnOrigin = (request: IncomingMessage): boolean => {
  const host = request.headers.host;

  if (host === undefined) {
    return false;
  }

  const hostname = new URL(`http://${host}`).hostname;

  if (!LOCAL_HOSTNAMES.includes(hostname)) {
    return false;
  }

  const origin = request.headers.origin;

  return origin === undefined || origin === `http://${host}`;
};

const readJson = (request: IncomingMessage): Promise<unknown> =>
  new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;

    request.on('data', (chunk: Buffer) => {
      size += chunk.length;

      if (size > BODY_LIMIT) {
        reject(new RangeError(`the request is larger than ${BODY_LIMIT} bytes`));
        request.destroy();

        return;
      }

      chunks.push(chunk);
    });
    request.on('end', () => {
      try {
        resolve(chunks.length === 0 ? null : JSON.parse(Buffer.concat(chunks).toString('utf8')));
      } catch (error) {
        reject(error);
      }
    });
    request.on('error', reject);
  });

/** Answers a request under `/api/`, and says whether it was one. */
export const handleApi = async (
  store: SampleStore,
  request: IncomingMessage,
  response: ServerResponse
): Promise<boolean> => {
  const { pathname } = new URL(request.url ?? '/', 'http://localhost');

  if (!pathname.startsWith(API_PREFIX)) {
    return false;
  }

  if (!isOwnOrigin(request)) {
    sendJson(response, 403, failure('FORBIDDEN', 'requests come only from the sample page'));

    return true;
  }

  if (
    request.method !== 'POST' ||
    !request.headers['content-type']?.startsWith('application/json')
  ) {
    sendJson(response, 415, failure('INVALID_ARGUMENT', 'send a POST request with a JSON body'));

    return true;
  }

  let args: unknown;

  try {
    args = await readJson(request);
  } catch (error) {
    sendJson(response, 400, failure('INVALID_ARGUMENT', String(error)));

    return true;
  }

  const method = pathname.slice(API_PREFIX.length);

  if (method !== 'seed') {
    sendJson(response, 200, await dispatch(store, method, args));

    return true;
  }

  response.writeHead(200, {
    'content-type': 'application/x-ndjson; charset=utf-8',
    'cache-control': 'no-store',
    'x-content-type-options': 'nosniff'
  });

  const reply = await dispatch(store, method, args, (progress) => {
    response.write(`${JSON.stringify({ progress })}\n`);
  });

  response.end(`${JSON.stringify({ reply })}\n`);

  return true;
};
