/**
 * Seeded randomness for the sample data, so that one seed always makes the
 * same objects. `randino` takes a function returning a number in `[0, 1)`
 * where it would otherwise call `Math.random`, and `Draw` is one of those
 * with the few helpers the generator needs.
 *
 * The generator is mulberry32, a small 32-bit generator in the public
 * domain: fast, and good enough for sample data, though not for anything
 * that needs to be unpredictable. `seedOf` mixes several numbers into one
 * seed with the finalizer of MurmurHash3, so that neighbouring record
 * numbers start from unrelated states. The Flutter sample has the same two
 * functions in `lib/src/random.dart`.
 */

const finalize = (value: number): number => {
  let h = value;

  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;

  return h >>> 0;
};

/** One 32-bit seed from several numbers, each taken as an unsigned 32-bit integer. */
export const seedOf = (...parts: number[]): number => {
  let h = 0x9e3779b9;

  for (const part of parts) {
    h = finalize((h ^ (part >>> 0)) + 0x7f4a7c15);
  }

  return h;
};

/** A generator of numbers in `[0, 1)`, the shape `randino` takes. */
export const mulberry32 = (seed: number): (() => number) => {
  let state = seed | 0;

  return () => {
    state = (state + 0x6d2b79f5) | 0;

    let t = Math.imul(state ^ (state >>> 15), 1 | state);

    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;

    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

/** Draws from one seed: numbers, picks and coin flips. */
export class Draw {
  readonly random: () => number;

  constructor(seed: number) {
    this.random = mulberry32(seed);
  }

  /** An integer from 0 up to, but not including, `bound`. */
  int(bound: number): number {
    return Math.floor(this.random() * bound);
  }

  /** An integer from `low` to `high`, both included. */
  between(low: number, high: number): number {
    return low + this.int(high - low + 1);
  }

  /** Whether an event of probability `p` happens. */
  chance(p: number): boolean {
    return this.random() < p;
  }

  pick<T>(values: readonly T[]): T {
    return values[this.int(values.length)];
  }

  /** `count` different values, or all of them when there are fewer. */
  picks<T>(values: readonly T[], count: number): T[] {
    const chosen = new Set<number>();
    const wanted = Math.min(count, values.length);

    while (chosen.size < wanted) {
      chosen.add(this.int(values.length));
    }

    return [...chosen].map((index) => values[index]);
  }

  /** A value of `values`, each as likely as its weight. */
  weighted<T>(values: readonly T[], weights: readonly number[]): T {
    let total = 0;

    for (const weight of weights) {
      total += weight;
    }

    let rest = this.random() * total;

    for (let index = 0; index < values.length; index += 1) {
      rest -= weights[index];

      if (rest < 0) {
        return values[index];
      }
    }

    return values[values.length - 1];
  }
}
