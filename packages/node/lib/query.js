'use strict';

/**
 * Building a query: `Query`, which a collection's `find` and `count` hand to
 * a function, and the conditions a filter is made of. A query becomes the IR
 * of `design/objects.md` in `codec.encodeQuery`, the same IR the query
 * language parses into, and the engine checks it against the schema.
 */

const { codeError } = require('./codec');

const OPS = Object.freeze({
  '==': 4,
  '!=': 5,
  '<': 6,
  '<=': 7,
  '>': 8,
  '>=': 9,
  between: 10,
  in: 11,
  contains: 12,
  startsWith: 13,
  endsWith: 14
});
const IS_NULL = 15;

function invalidQuery(message) {
  return codeError('INVALID_QUERY', message);
}

/** A field's path: its name, or names joined by `.`, or an array of names. */
function pathOf(field) {
  if (typeof field === 'string') {
    return field.split('.');
  }

  if (Array.isArray(field) && field.every((name) => typeof name === 'string')) {
    return field.slice();
  }

  throw invalidQuery('a field is named by a string, or an array of names');
}

/** A condition, as the filter holds it. */
class Condition {
  constructor(node) {
    this.node = node;
    Object.freeze(this);
  }
}

function test(op, field, values) {
  const path = pathOf(field);

  // Equal to null is a null test, and so is different from null.
  if ((op === OPS['=='] || op === OPS['!=']) && values[0] === null) {
    const isNull = { kind: 'test', op: IS_NULL, path, values: [] };

    return new Condition(op === OPS['=='] ? isNull : { kind: 'not', term: isNull });
  }

  return new Condition({ kind: 'test', op, path, values });
}

function nodeOf(condition) {
  if (!(condition instanceof Condition)) {
    throw invalidQuery('a filter is made of conditions');
  }

  return condition.node;
}

/** Terms of an `and` or an `or`, with nested ones of the same kind taken apart. */
function flatten(kind, conditions) {
  const terms = [];

  for (const condition of conditions) {
    const node = nodeOf(condition);

    if (node.kind === kind) {
      terms.push(...node.terms);
    } else {
      terms.push(node);
    }
  }

  return new Condition({ kind, terms });
}

/** The conditions a filter is made of. */
const conditions = Object.freeze({
  eq: (field, value) => test(OPS['=='], field, [value]),
  ne: (field, value) => test(OPS['!='], field, [value]),
  lt: (field, value) => test(OPS['<'], field, [value]),
  le: (field, value) => test(OPS['<='], field, [value]),
  gt: (field, value) => test(OPS['>'], field, [value]),
  ge: (field, value) => test(OPS['>='], field, [value]),
  between: (field, low, high) => test(OPS.between, field, [low, high]),
  in: (field, values) => {
    if (!Array.isArray(values)) {
      throw invalidQuery('`in` takes an array of values');
    }

    return test(OPS.in, field, values.slice());
  },
  contains: (field, value) => test(OPS.contains, field, [value]),
  startsWith: (field, value) => test(OPS.startsWith, field, [value]),
  endsWith: (field, value) => test(OPS.endsWith, field, [value]),
  isNull: (field) => test(IS_NULL, field, []),
  isNotNull: (field) => new Condition({ kind: 'not', term: test(IS_NULL, field, []).node }),
  and: (...terms) => flatten('and', terms),
  or: (...terms) => flatten('or', terms),
  not: (condition) => new Condition({ kind: 'not', term: nodeOf(condition) })
});

/** The condition `where` describes: a field, an operator and a value. */
function conditionOf(field, op, value) {
  switch (op) {
    case 'between':
      if (!Array.isArray(value) || value.length !== 2) {
        throw invalidQuery('`between` takes the low and the high value, as a pair');
      }

      return conditions.between(field, value[0], value[1]);
    case 'in':
      return conditions.in(field, value);
    default:
      if (!Object.hasOwn(OPS, op)) {
        throw invalidQuery(`\`${String(op)}\` is not an operator`);
      }

      return test(OPS[op], field, [value]);
  }
}

/**
 * What to find, in what order, and how many. Each call adds to the query
 * and returns it.
 */
class Query {
  #filter = null;
  #sort = [];
  #offset = 0;
  #limit = null;

  /**
   * Keeps the objects that meet a condition, and every condition given
   * before: `where('age', '>=', 18)`, a condition, or a function that makes
   * one from `conditions`.
   */
  where(field, op, value) {
    let condition;

    if (typeof field === 'function') {
      condition = field(conditions);
    } else if (field instanceof Condition) {
      condition = field;
    } else {
      condition = conditionOf(field, op, value);
    }

    const node = nodeOf(condition);

    this.#filter =
      this.#filter === null ? node : flatten('and', [new Condition(this.#filter), condition]).node;

    return this;
  }

  /** Sorts by `field`, `'asc'` or `'desc'`, after any sort given before. */
  sortBy(field, direction = 'asc') {
    if (direction !== 'asc' && direction !== 'desc') {
      throw invalidQuery('a sort is `asc` or `desc`');
    }

    this.#sort.push([pathOf(field), direction === 'desc']);

    return this;
  }

  /** Returns at most `count` objects. */
  limit(count) {
    this.#limit = count;

    return this;
  }

  /** Skips the first `count` objects. */
  offset(count) {
    this.#offset = count;

    return this;
  }

  /** The query as `codec.encodeQuery` takes it. */
  parts() {
    for (const [name, value] of [
      ['offset', this.#offset],
      ['limit', this.#limit ?? 0]
    ]) {
      if (!Number.isSafeInteger(value) || value < 0) {
        throw invalidQuery(`a query's ${name} is a whole number from 0 up`);
      }
    }

    return { filter: this.#filter, sort: this.#sort, offset: this.#offset, limit: this.#limit };
  }
}

module.exports = { Query, Condition, conditions };
