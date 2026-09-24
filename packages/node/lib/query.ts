/**
 * Building a query: `Query`, which a collection's `find` and `count` hand to
 * a function, and the conditions a filter is made of. A query becomes the IR
 * of `design/objects.md` in `codec.encodeQuery`, the same IR the query
 * language parses into, and the engine checks it against the schema.
 */

import { Param, codeError } from './codec.js';
import type { CodeError } from './codec.js';

/**
 * A node of a query's filter: a test of a field, with its operator's number
 * and the values it compares with, or `and`, `or` or `not` of other nodes.
 */
export type FilterNode =
  | { kind: 'test'; op: number; path: string[]; values: unknown[] }
  | { kind: 'and' | 'or'; terms: FilterNode[] }
  | { kind: 'not'; term: FilterNode };

/**
 * A query as `Query.parts` gives it and `codec.encodeQuery` takes it: its
 * filter, its sort as paths each with whether it descends, its offset, and
 * its limit, `null` for none.
 */
export interface QueryParts {
  filter: FilterNode | null;
  sort: [string[], boolean][];
  offset: number;
  limit: number | null;
}

/** The conditions a filter is made of, as a function given to `where` gets them. */
export type Conditions = typeof conditions;

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

function invalidQuery(message: string): CodeError {
  return codeError('INVALID_QUERY', message);
}

/** A field's path: its name, or names joined by `.`, or an array of names. */
function pathOf(field: unknown): string[] {
  if (typeof field === 'string') {
    return field.split('.');
  }

  if (Array.isArray(field) && field.every((name) => typeof name === 'string')) {
    return field.slice();
  }

  throw invalidQuery('a field is named by a string, or an array of names');
}

/**
 * A parameter in place of a value, for a query that `Database.prepare`
 * prepares once and each run gives values: `param(0)` is the first.
 */
function param(index: number): Param {
  if (!Number.isSafeInteger(index) || index < 0) {
    throw invalidQuery("a parameter's number is a whole number from 0 up");
  }

  return new Param(index);
}

/** A condition, as the filter holds it. */
class Condition {
  declare readonly node: FilterNode;

  constructor(node: FilterNode) {
    this.node = node;
    Object.freeze(this);
  }
}

function test(op: number, field: unknown, values: unknown[]): Condition {
  const path = pathOf(field);

  // Equal to null is a null test, and so is different from null.
  if ((op === OPS['=='] || op === OPS['!=']) && values[0] === null) {
    const isNull: FilterNode = { kind: 'test', op: IS_NULL, path, values: [] };

    return new Condition(op === OPS['=='] ? isNull : { kind: 'not', term: isNull });
  }

  return new Condition({ kind: 'test', op, path, values });
}

function nodeOf(condition: unknown): FilterNode {
  if (!(condition instanceof Condition)) {
    throw invalidQuery('a filter is made of conditions');
  }

  return condition.node;
}

/** Terms of an `and` or an `or`, with nested ones of the same kind taken apart. */
function flatten(kind: 'and' | 'or', conditions: readonly unknown[]): Condition {
  const terms: FilterNode[] = [];

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
  eq: (field: unknown, value: unknown) => test(OPS['=='], field, [value]),
  ne: (field: unknown, value: unknown) => test(OPS['!='], field, [value]),
  lt: (field: unknown, value: unknown) => test(OPS['<'], field, [value]),
  le: (field: unknown, value: unknown) => test(OPS['<='], field, [value]),
  gt: (field: unknown, value: unknown) => test(OPS['>'], field, [value]),
  ge: (field: unknown, value: unknown) => test(OPS['>='], field, [value]),
  between: (field: unknown, low: unknown, high: unknown) => test(OPS.between, field, [low, high]),
  in: (field: unknown, values: unknown) => {
    if (!Array.isArray(values)) {
      throw invalidQuery('`in` takes an array of values');
    }

    return test(OPS.in, field, values.slice());
  },
  contains: (field: unknown, value: unknown) => test(OPS.contains, field, [value]),
  startsWith: (field: unknown, value: unknown) => test(OPS.startsWith, field, [value]),
  endsWith: (field: unknown, value: unknown) => test(OPS.endsWith, field, [value]),
  isNull: (field: unknown) => test(IS_NULL, field, []),
  isNotNull: (field: unknown) =>
    new Condition({ kind: 'not', term: test(IS_NULL, field, []).node }),
  and: (...terms: unknown[]) => flatten('and', terms),
  or: (...terms: unknown[]) => flatten('or', terms),
  not: (condition: unknown) => new Condition({ kind: 'not', term: nodeOf(condition) })
});

/** The condition `where` describes: a field, an operator and a value. */
function conditionOf(field: unknown, op: unknown, value: unknown): Condition {
  switch (op) {
    case 'between':
      if (!Array.isArray(value) || value.length !== 2) {
        throw invalidQuery('`between` takes the low and the high value, as a pair');
      }

      return conditions.between(field, value[0], value[1]);
    case 'in':
      return conditions.in(field, value);
    default:
      // `hasOwn` takes any value as a name, as reading a property does.
      if (!Object.hasOwn(OPS, op as PropertyKey)) {
        throw invalidQuery(`\`${String(op)}\` is not an operator`);
      }

      // `hasOwn` above holds it to a name of `OPS`.
      return test(OPS[op as keyof typeof OPS], field, [value]);
  }
}

/**
 * What to find, in what order, and how many. Each call adds to the query
 * and returns it.
 */
class Query {
  #filter: FilterNode | null = null;
  #sort: [string[], boolean][] = [];
  #offset = 0;
  #limit: number | null = null;

  /**
   * Keeps the objects that meet a condition, and every condition given
   * before: `where('age', '>=', 18)`, a condition, or a function that makes
   * one from `conditions`.
   */
  where(
    field: string | readonly string[] | Condition | ((conditions: Conditions) => unknown),
    op?: unknown,
    value?: unknown
  ): this {
    let condition: unknown;

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
  sortBy(field: unknown, direction: unknown = 'asc'): this {
    if (direction !== 'asc' && direction !== 'desc') {
      throw invalidQuery('a sort is `asc` or `desc`');
    }

    this.#sort.push([pathOf(field), direction === 'desc']);

    return this;
  }

  /** Returns at most `count` objects. */
  limit(count: number): this {
    this.#limit = count;

    return this;
  }

  /** Skips the first `count` objects. */
  offset(count: number): this {
    this.#offset = count;

    return this;
  }

  /** The query as `codec.encodeQuery` takes it. */
  parts(): QueryParts {
    for (const [name, value] of [
      ['offset', this.#offset],
      ['limit', this.#limit ?? 0]
    ] as const) {
      if (!Number.isSafeInteger(value) || value < 0) {
        throw invalidQuery(`a query's ${name} is a whole number from 0 up`);
      }
    }

    return { filter: this.#filter, sort: this.#sort, offset: this.#offset, limit: this.#limit };
  }
}

export { Query, Condition, Param, conditions, param };
