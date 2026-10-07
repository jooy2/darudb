/**
 * Deletes the web server's database folder once every test has run. The
 * server may still hold the file open, which Windows refuses to delete, so
 * a folder left behind is reported rather than failing the run.
 */
import { rmSync } from 'node:fs';

export default (): void => {
  const directory = process.env.DARUDB_SAMPLE_WEB_DIR;

  if (directory === undefined) {
    return;
  }

  try {
    rmSync(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  } catch (error) {
    console.warn(`could not delete ${directory}: ${String(error)}`);
  }
};
