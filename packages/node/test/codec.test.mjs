/**
 * The records `lib/codec.js` writes and reads, on their own: every type
 * round-trips, fields a schema no longer has are skipped whatever their type,
 * and bytes that are not a record fail with `CORRUPTED` and nothing else,
 * since the records it reads come from a file.
 */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { describe, it } from 'node:test';

const require = createRequire(import.meta.url);
const { decodeRecords, encodeRecords } = require('../dist/codec.js');

/** A layout as `decodeSchema` builds one, from fields in id order. */
const layoutOf = (list) => {
  const byId = new Uint16Array((list.at(-1)?.id ?? 0) + 1);

  list.forEach((field, index) => {
    byId[field.id] = index + 1;
  });

  return {
    name: 'things',
    fields: {
      list,
      positions: new Map(list.map((field, index) => [field.id, index])),
      byId,
      names: new Set(list.map((field) => field.name)),
      hasProto: list.some((field) => field.name === '__proto__')
    }
  };
};

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

  it('read the strings of records from a Buffer, as the engine gives them', () => {
    // Strings short and long, ASCII and not, in embedded objects too, in
    // records short enough for their strings to be cut from one text of the
    // record and in one too long for that; records one after another, so
    // that each one's strings come from its own bytes.
    const texts = layoutOf([
      field(1, 'a', { type: 'string' }),
      field(2, 'b', { type: 'string' }),
      field(3, 'c', { type: 'bytes' }),
      field(4, 'd', {
        type: 'object',
        fields: layoutOf([field(1, 'inner', { type: 'string' })]).fields
      })
    ]);
    const objects = [
      { a: 'person 1', b: '1@example.com', c: new Uint8Array([200, 0, 128]), d: { inner: 'x' } },
      { a: 'p', b: '', c: new Uint8Array(0), d: { inner: 'one, then another' } },
      { a: 'héllo', b: 'city 2', c: new Uint8Array([255]), d: { inner: 'wörld' } },
      { a: 'a'.repeat(64), b: 'b'.repeat(65), c: new Uint8Array(3), d: { inner: 'inner' } },
      { a: 'short', b: 'x'.repeat(600), c: new Uint8Array(700).fill(233), d: { inner: 'last' } },
      { a: 'after the long one', b: 'y', c: new Uint8Array([1]), d: { inner: 'z' } },
      // ASCII long enough to grow the buffer before a character that is not.
      {
        a: `${'a'.repeat(300)}é`,
        b: 'z',
        c: new Uint8Array(0),
        d: { inner: `${'b'.repeat(280)}ü` }
      }
    ];
    const bytes = encodeRecords(texts, objects);

    assert.deepEqual(decodeRecords(texts, Buffer.from(bytes)), objects);
    assert.deepEqual(decodeRecords(texts, bytes), objects);
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

  it('read every field under its own name, whatever the name holds', () => {
    // Names from a file, which the generated code that makes objects must
    // hold as names and never run.
    const names = [
      'x": (globalThis.injected = true), "y',
      '}); globalThis.injected = true; ({',
      '"; globalThis.injected = true; "',
      "'; globalThis.injected = true; '",
      '\\',
      'a"b\\c',
      '\u2028line\u2029',
      '\ud800',
      '',
      'constructor',
      '0',
      'values'
    ];
    const odd = layoutOf(names.map((name, index) => field(index + 1, name, { type: 'int' })));
    const object = Object.fromEntries(names.map((name, index) => [name, index]));
    const [back] = decodeRecords(odd, encodeRecords(odd, [object]));

    assert.equal(globalThis.injected, undefined);
    assert.equal(Object.getPrototypeOf(back), Object.prototype);
    assert.deepEqual(Object.keys(back).sort(), [...names].sort());

    for (const [index, name] of names.entries()) {
      assert.equal(Object.getOwnPropertyDescriptor(back, name)?.value, index, name);
    }
  });

  it('read a field named `__proto__` as a field, not as the prototype', () => {
    const proto = layoutOf([
      field(1, '__proto__', { type: 'string' }),
      field(2, 'x', { type: 'int' })
    ]);
    const object = { x: 1 };

    Object.defineProperty(object, '__proto__', { value: 'p', enumerable: true });

    const [back] = decodeRecords(proto, encodeRecords(proto, [object]));

    assert.equal(Object.getPrototypeOf(back), Object.prototype);
    assert.equal(Object.hasOwn(back, '__proto__'), true);
    assert.equal(back.x, 1);
  });

  it('read records where a process forbids making code from strings', () => {
    const script = `
      const { decodeRecords, encodeRecords } = require(${JSON.stringify(require.resolve('../dist/codec.js'))});
      const list = [{ id: 1, name: 'a', kind: { type: 'int' }, optional: true, default: undefined }];
      const layout = { name: 't', fields: { list, positions: new Map([[1, 0]]), byId: null, names: new Set(['a']), hasProto: false } };
      process.stdout.write(JSON.stringify(decodeRecords(layout, encodeRecords(layout, [{ a: 7 }]))));
    `;
    const output = execFileSync(process.execPath, [
      '--disallow-code-generation-from-strings',
      '-e',
      script
    ]);

    assert.equal(output.toString(), '[{"a":7}]');
  });

  it('write, from the code made for a layout, what walking the layout writes', () => {
    // The same layouts with `encode` set to `null` at every level, which
    // `writeFields` walks rather than making code for.
    const walking = (fields) => ({
      ...fields,
      encode: null,
      list: fields.list.map((field) =>
        field.kind.type === 'object'
          ? { ...field, kind: { ...field.kind, fields: walking(field.kind.fields) } }
          : field
      )
    });
    const walked = { ...everything, fields: walking(everything.fields) };
    const next = random(7);
    const pick = (values) => values[Math.floor(next() * values.length)];
    const valid = {
      bool: () => pick([true, false]),
      int: () => pick([0, 1, -1, 127, 2 ** 53 - 1, -(2 ** 40), 2n ** 60n, -5n]),
      float: () => pick([0, -0.5, 1e300, Number.NaN, 3]),
      string: () => pick(['', 'a', 'héllo', 'x'.repeat(200)]),
      bytes: () => pick([new Uint8Array(0), new Uint8Array([1, 2, 255])]),
      link: () => pick(['north', '']),
      list: () => pick([[], [1, -2], [2 ** 53 - 1]]),
      object: () => pick([{ inner: 'x' }, {}, { inner: null }])
    };
    // Values of the wrong type, or of the right type out of range.
    const wrong = () =>
      pick([1.5, '1', 1, 2n ** 64n, 2n ** 62n, true, [1], { inner: 3 }, new Uint16Array(2), 'x']);
    const outcome = (layout, objects) => {
      try {
        return Buffer.from(encodeRecords(layout, objects)).toString('hex');
      } catch (error) {
        return `${error.code}: ${error.message}`;
      }
    };
    let refused = 0;

    for (let round = 0; round < 2000; round++) {
      const object = next() < 0.1 ? Object.create({ inherited: 1, int: 5 }) : {};

      for (const field of everything.fields.list) {
        const roll = next();

        if (roll < 0.2) {
          continue;
        }

        object[field.name] =
          roll < 0.3 ? pick([null, undefined]) : roll < 0.35 ? wrong() : valid[field.name]();
      }

      if (next() < 0.05) {
        object.extra = 1;
      }

      if (next() < 0.05) {
        Object.defineProperty(object, 'hidden', { value: 1, enumerable: false });
      }

      if (next() < 0.05) {
        Object.defineProperty(object, 'string', { get: () => 'from a getter', enumerable: true });
      }

      const made = outcome(everything, [object]);

      assert.equal(
        made,
        outcome(walked, [object]),
        JSON.stringify(object, (_, v) => String(v))
      );
      refused += made.startsWith('INVALID_ARGUMENT') ? 1 : 0;
    }

    assert.ok(refused > 50 && refused < 1500, `${refused} refused`);
  });

  it('read, with the code made for a layout, what reading field by field reads', () => {
    // The same layout with `decode` set to `null`, which `readFields` reads
    // field by field rather than making code for; and one with a required
    // field, a default, a `t.bigint()` int and an int of any size.
    const read = (layout) => ({ ...layout, fields: { ...layout.fields, decode: null } });
    const typed = layoutOf([
      { ...field(1, 'name', { type: 'string' }, false) },
      { ...field(2, 'age', { type: 'int' }, false), default: 18 },
      field(3, 'big', { type: 'int', big: true }),
      field(4, 'any', { type: 'int', anyInt: true }),
      field(6, 'flag', { type: 'bool' })
    ]);
    const next = random(11);
    const outcome = (layout, bytes) => {
      try {
        return decodeRecords(layout, bytes);
      } catch (error) {
        return `${error.code}: ${error.message}`;
      }
    };
    let damaged = 0;

    for (let round = 0; round < 3000; round++) {
      const layout = round % 2 === 0 ? everything : typed;
      const objects =
        layout === everything
          ? [sample, { int: 2 ** 53 - 1, string: 'x' }, {}]
          : [
              { name: 'a', big: 5n, any: 2n ** 60n, flag: true },
              { name: 'b', age: 3 }
            ];
      const bytes = Buffer.from(encodeRecords(layout, objects));

      // Most records changed a little: a byte, or cut short.
      if (next() < 0.8 && bytes.length > 0) {
        const at = Math.floor(next() * bytes.length);

        if (next() < 0.2) {
          bytes.fill(0, at);
        } else {
          bytes[at] = next() < 0.5 ? Math.floor(next() * 12) : Math.floor(next() * 256);
        }
      }

      const made = outcome(layout, bytes);

      assert.deepEqual(made, outcome(read(layout), bytes), `${bytes.toString('hex')}`);
      damaged += typeof made === 'string' ? 1 : 0;
    }

    assert.ok(damaged > 500 && damaged < 2800, `${damaged} damaged`);
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
    const { encodeSchema, decodeSchema } = require('../dist/codec.js');
    const { collection, schema, t } = require('../dist/schema.js');
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
