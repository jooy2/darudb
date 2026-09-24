/**
 * The records `lib/codec.js` writes and reads, on their own: every type
 * round-trips, fields a schema no longer has are skipped whatever their type,
 * and bytes that are not a record fail with `CORRUPTED` and nothing else,
 * since the records it reads come from a file.
 */
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { describe, it } from 'node:test';

const require = createRequire(import.meta.url);
const { decodeRecords, encodeRecords } = require('../lib/codec.js');

/** A layout as `decodeSchema` builds one, from fields in id order. */
const layoutOf = (list) => ({
  name: 'things',
  fields: {
    list,
    positions: new Map(list.map((field, index) => [field.id, index])),
    names: new Set(list.map((field) => field.name)),
    hasProto: false
  }
});

const field = (id, name, kind, optional = true) => ({
  id,
  name,
  kind,
  optional,
  default: undefined
});

const keyed = layoutOf([field(1, 'key', { type: 'string' }, false)]);

const everything = layoutOf([
  field(1, 'bool', { type: 'bool' }),
  field(2, 'int', { type: 'int' }),
  field(3, 'float', { type: 'float' }),
  field(4, 'string', { type: 'string' }),
  field(5, 'bytes', { type: 'bytes' }),
  field(6, 'link', { type: 'link', target: { key: keyed.fields.list[0] } }),
  field(7, 'list', { type: 'list', element: { type: 'int' } }),
  field(8, 'object', {
    type: 'object',
    fields: layoutOf([field(1, 'inner', { type: 'string' })]).fields
  })
]);

const sample = {
  bool: true,
  int: -123456789,
  float: -0.5,
  string: 'héllo\u0000wörld',
  bytes: new Uint8Array([0, 255, 7]),
  link: 'north',
  list: [0, 1, -1, 2 ** 53 - 1, -(2 ** 53 - 1)],
  object: { inner: 'x' }
};

/** A tiny deterministic generator, so a failure replays. */
const random = (seed) => () => {
  seed = (seed * 1103515245 + 12345) % 2147483648;

  return seed / 2147483648;
};

describe('records', () => {
  it('round-trip every type', () => {
    const [back] = decodeRecords(everything, encodeRecords(everything, [sample]));

    assert.deepEqual(back, sample);
  });

  it('keep ints exact across the whole 64-bit range', () => {
    const ints = layoutOf([field(1, 'n', { type: 'int', anyInt: true })]);
    const values = [
      0,
      1,
      -1,
      2 ** 52,
      -(2 ** 52),
      2 ** 53 - 1,
      -(2 ** 53 - 1),
      2n ** 60n,
      -(2n ** 63n),
      2n ** 63n - 1n
    ];
    const back = decodeRecords(
      ints,
      encodeRecords(
        ints,
        values.map((n) => ({ n }))
      )
    );

    assert.deepEqual(
      back.map((object) => object.n),
      values
    );
  });

  it('skip fields the schema no longer has, whatever their type', () => {
    // Every field of `everything` read with a schema that has none of them
    // but the last.
    const later = layoutOf([
      field(8, 'object', {
        type: 'object',
        fields: layoutOf([field(1, 'inner', { type: 'string' })]).fields
      })
    ]);
    const [back] = decodeRecords(later, encodeRecords(everything, [sample]));

    assert.deepEqual(back, { object: { inner: 'x' } });
  });

  it('read bytes that are not a record as damage, and as nothing else', () => {
    const next = random(42);

    for (let round = 0; round < 3000; round++) {
      const bytes = new Uint8Array(Math.floor(next() * 40));

      for (let index = 0; index < bytes.length; index++) {
        // Mostly small values, so that lengths and tags are often plausible.
        bytes[index] = next() < 0.7 ? Math.floor(next() * 12) : Math.floor(next() * 256);
      }

      try {
        decodeRecords(everything, bytes);
      } catch (error) {
        assert.equal(error.code, 'CORRUPTED', `${bytes}: ${error.stack}`);
      }
    }
  });

  it('refuse a value of another type with the field it is in', () => {
    assert.throws(() => encodeRecords(everything, [{ int: 1.5 }]), {
      code: 'INVALID_ARGUMENT',
      message: /`int` holds an int/
    });
    assert.throws(() => encodeRecords(everything, [{ object: { inner: 3 } }]), {
      code: 'INVALID_ARGUMENT',
      message: /`object.inner`/
    });
  });
});

describe('the stored schema', () => {
  it('reads bytes that are not a schema as damage, and as nothing else', () => {
    const { encodeSchema, decodeSchema } = require('../lib/codec.js');
    const { collection, schema, t } = require('../lib/schema.js');
    const bytes = encodeSchema(
      schema(1, {
        teams: collection({ name: t.string().primaryKey() }),
        users: collection({
          name: t.string().default('x'),
          tags: t.list(t.string()).optional().index(),
          team: t.link('teams').optional(),
          address: t.object({ city: t.string() }).optional()
        })
      })
    );
    const next = random(7);

    for (let round = 0; round < 5000; round++) {
      const damaged = bytes.slice();

      for (let flips = 1 + Math.floor(next() * 3); flips > 0; flips--) {
        damaged[Math.floor(next() * damaged.length)] = Math.floor(next() * 256);
      }

      try {
        decodeSchema(damaged);
      } catch (error) {
        assert.equal(error.code, 'CORRUPTED', error.stack);
      }
    }
  });
});
