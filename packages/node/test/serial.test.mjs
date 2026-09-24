/**
 * `Serial`, which sends an asynchronous transaction's operations to the
 * engine in batches, tested against a stand-in for the native transaction
 * whose batches settle when the test says so. The order of batches is
 * otherwise up to the thread pool, which runs them in the order they arrive
 * nearly always, so only a stand-in shows a batch sent too early.
 */
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { describe, it } from 'node:test';

const require = createRequire(import.meta.url);
const { Serial } = require('../lib/async.js');

const COUNT = 3;
const TAG_NUMBER = 2;
const TAG_FAILURE = 6;

/** A native transaction whose batches wait for the test to settle them. */
class Stand {
  batches = [];

  runAsync(kinds) {
    return new Promise((resolve) => {
      this.batches.push({ kinds: [...kinds], resolve });
    });
  }
}

/** A batch's results: numbers, or failures given as `[code, message]`. */
const results = (...values) =>
  Buffer.concat(
    values.map((value) => {
      if (typeof value === 'number') {
        const bytes = Buffer.alloc(9);

        bytes[0] = TAG_NUMBER;
        bytes.writeDoubleLE(value, 1);

        return bytes;
      }

      const [code, message] = value.map((text) => Buffer.from(text));

      return Buffer.concat([
        Buffer.from([TAG_FAILURE, code.length]),
        code,
        Buffer.from([message.length]),
        message
      ]);
    })
  );

/** Lets queued microtasks run, and the batch they send go out. */
const turn = () => new Promise((resolve) => setImmediate(resolve));

const count = (serial) => serial.call(COUNT, '', null, Buffer.alloc(0));

describe('Serial', () => {
  it('sends the operations of one turn together, and those of later turns after them', async () => {
    const stand = new Stand();
    const serial = new Serial(stand);
    const first = [count(serial), count(serial), count(serial)];

    await turn();
    assert.equal(stand.batches.length, 1);
    assert.deepEqual(stand.batches[0].kinds, [COUNT, COUNT, COUNT]);

    const second = [count(serial), count(serial)];

    await turn();
    assert.equal(stand.batches.length, 1, 'nothing goes while a batch is out');

    stand.batches[0].resolve(results(1, 2, 3));
    assert.deepEqual(await Promise.all(first), [1, 2, 3]);
    await turn();
    assert.equal(stand.batches.length, 2);
    assert.deepEqual(stand.batches[1].kinds, [COUNT, COUNT]);

    stand.batches[1].resolve(results(4, 5));
    assert.deepEqual(await Promise.all(second), [4, 5]);
  });

  it('fails only the operation that failed, or every one when the batch did', async () => {
    const stand = new Stand();
    const serial = new Serial(stand);
    const batch = [count(serial), count(serial), count(serial)];

    await turn();
    stand.batches[0].resolve(results(1, ['DUPLICATE_KEY', 'taken'], 3));

    const settled = await Promise.allSettled(batch);

    assert.deepEqual(
      settled.map((outcome) => outcome.value ?? outcome.reason.code),
      [1, 'DUPLICATE_KEY', 3]
    );
    assert.equal(settled[1].reason.message, 'taken');

    const failing = [count(serial), count(serial)];

    await turn();
    stand.batches[1].resolve({ code: 'CLOSED', message: 'the transaction has ended' });

    for (const outcome of await Promise.allSettled(failing)) {
      assert.equal(outcome.reason.code, 'CLOSED');
    }

    const after = count(serial);

    await turn();
    stand.batches[2].resolve(results(7));
    assert.equal(await after, 7, 'a failed batch does not stop the next');
  });

  it('drains once every operation called has settled, and takes none after closing', async () => {
    const stand = new Stand();
    const serial = new Serial(stand);
    let drained = false;

    await serial.drain();

    const pending = count(serial);

    serial.drain().then(() => {
      drained = true;
    });
    await turn();
    assert.equal(drained, false);

    const closing = serial.close();

    await assert.rejects(count(serial), (error) => error.code === 'CLOSED');
    stand.batches[0].resolve(results(1));
    await closing;
    assert.equal(drained, true);
    assert.equal(await pending, 1);
    assert.equal(stand.batches.length, 1);
  });
});
