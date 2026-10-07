/**
 * How the screens reach the database, which they never hold themselves.
 * In Electron the preload script gives the page `window.sample`, a bridge
 * to the main process over IPC; in a browser the page asks the server that
 * served it, over HTTP. Both carry the same methods and replies, from
 * `core/protocol.ts`, and a failed reply becomes a `CallError` with the
 * engine's code, such as `DUPLICATE_KEY`.
 */
import type {
  Host,
  Method,
  Methods,
  Reply,
  SeedOptions,
  SeedProgress,
  SeedReport
} from '../core/protocol.ts';

/** What `electron/preload.cjs` puts on the page. */
interface Bridge {
  call(method: Method, args: unknown): Promise<Reply<unknown>>;
  onProgress(listener: (progress: SeedProgress) => void): () => void;
}

declare global {
  interface Window {
    sample?: Bridge;
  }
}

export class CallError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

export interface Backend {
  readonly host: Host;
  call<M extends Method>(method: M, args: Methods[M]['args']): Promise<Methods[M]['result']>;
  /** Runs `seed`, calling `onProgress` as each batch commits. */
  seed(options: SeedOptions, onProgress: (progress: SeedProgress) => void): Promise<SeedReport>;
}

const unwrap = <T>(reply: Reply<T>): T => {
  if (reply.ok) {
    return reply.value;
  }

  throw new CallError(reply.code, reply.message);
};

const electronBackend = (bridge: Bridge): Backend => ({
  host: 'electron',
  call: async (method, args) => unwrap(await bridge.call(method, args)) as never,
  seed: async (options, onProgress) => {
    const stop = bridge.onProgress(onProgress);

    try {
      return unwrap(await bridge.call('seed', options)) as SeedReport;
    } finally {
      stop();
    }
  }
});

const post = (method: Method, args: unknown): Promise<Response> =>
  fetch(`api/${method}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args)
  });

/** The lines of a response of one JSON value per line, as they arrive. */
async function* linesOf(response: Response): AsyncGenerator<unknown> {
  if (response.body === null) {
    return;
  }

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';

  for (;;) {
    const { done, value } = await reader.read();

    if (done) {
      break;
    }

    buffer += value;

    let end = buffer.indexOf('\n');

    while (end >= 0) {
      const line = buffer.slice(0, end).trim();

      buffer = buffer.slice(end + 1);

      if (line.length > 0) {
        yield JSON.parse(line);
      }

      end = buffer.indexOf('\n');
    }
  }
}

const webBackend = (): Backend => ({
  host: 'web',
  call: async (method, args) => unwrap((await (await post(method, args)).json()) as Reply<never>),
  seed: async (options, onProgress) => {
    let reply: Reply<SeedReport> | null = null;

    for await (const line of linesOf(await post('seed', options))) {
      const message = line as { progress?: SeedProgress; reply?: Reply<SeedReport> };

      if (message.progress !== undefined) {
        onProgress(message.progress);
      } else if (message.reply !== undefined) {
        reply = message.reply;
      }
    }

    if (reply === null) {
      throw new CallError('INTERNAL', 'the server ended the run without a reply');
    }

    return unwrap(reply);
  }
});

export const backend: Backend =
  window.sample === undefined ? webBackend() : electronBackend(window.sample);

/** The code and message of an error, for a toast. */
export const describeError = (error: unknown): { code: string; message: string } =>
  error instanceof CallError
    ? { code: error.code, message: error.message }
    : { code: 'INTERNAL', message: error instanceof Error ? error.message : String(error) };
