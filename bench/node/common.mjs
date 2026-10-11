// What every store runs: the objects, the order random reads go in, timing,
// and the digest a row's results are checked with. bench/README.md describes
// the workloads, and the other languages' harnesses draw the same objects in
// the same order.

export const OBJECTS = 100_000;

/** The `n`th object, counting from 0. It gets the key `n + 1`. */
export const person = (n) => ({
  name: `person ${n}`,
  email: `${n}@example.com`,
  age: (n * 7919) % 80,
  city: `city ${n % 100}`,
  score: (n * 0.618) % 1
});

const SPREAD = 0x9e3779b97f4a7c15n;
const MASK = (1n << 64n) - 1n;

/** Spreads consecutive numbers over 64 bits, as the Rust harness does. */
const scatter = (round) => {
  const x = (BigInt(round) * SPREAD) & MASK;

  return ((x << 17n) | (x >> 47n)) & MASK;
};

/** The keys the random reads ask for, drawn before the timing starts. */
export const randomIds = (count) =>
  Array.from({ length: count }, (_, round) => 1 + Number(scatter(round) % BigInt(OBJECTS)));

/** The numbers of the objects the random email lookups ask for. */
export const randomNumbers = (count) =>
  Array.from({ length: count }, (_, round) => Number(scatter(round) % BigInt(OBJECTS)));

/** How many results a row saw, and a 32-bit hash of their keys and ages. */
export class Digest {
  count = 0;
  hash = 0;

  add(id, age) {
    this.count++;
    this.hash = Math.imul(this.hash ^ (id * 131 + age), 0x01000193) >>> 0;
  }

  person(p) {
    this.add(p.id, p.age);
  }

  number(n) {
    this.add(n, 0);
  }
}

/**
 * The rows whose first run is a row of its own as well: queries a store may
 * answer faster once it has run them, which an application that runs one
 * once does not see.
 */
export const FIRST_RUN = { 'city-scan': 'city-scan-first', 'top-score': 'top-score-first' };

export class Rows {
  rows = [];

  /**
   * Runs `step` `count` times and records the time each took on average, and
   * the first run's time on its own for a row of `FIRST_RUN`, whose digest
   * then leaves the first run out.
   */
  each(row, count, step) {
    let digest = new Digest();
    const started = process.hrtime.bigint();
    let round = 0;

    if (row in FIRST_RUN) {
      step(round++, digest);
      this.push(FIRST_RUN[row], Number(process.hrtime.bigint() - started), digest);
      digest = new Digest();
    }

    for (; round < count; round++) {
      step(round, digest);
    }

    this.push(row, Number(process.hrtime.bigint() - started) / count, digest);
  }

  /** Runs `work` once, which does `count` operations and commits them. */
  all(row, count, work) {
    const digest = new Digest();
    const started = process.hrtime.bigint();

    work(digest);
    this.push(row, Number(process.hrtime.bigint() - started) / count, digest);
  }

  /** A row the store has no way to do, which the table leaves empty. */
  none(row) {
    this.rows.push({ row, ns: null, count: 0, hash: 0 });
  }

  /** A row that is checked and not timed. */
  check(row, digest) {
    this.push(row, 0, digest);
  }

  push(row, ns, digest) {
    this.rows.push({ row, ns, count: digest.count, hash: digest.hash });
  }
}

/** Plain objects, which every store's reads are made into. */
export const plain = (id, o) => ({
  id,
  name: o.name,
  email: o.email,
  age: o.age,
  city: o.city,
  score: o.score
});

/** Writes the rows for the parent, one line of JSON each, and exits. */
export const finish = (rows) => {
  process.stdout.write(rows.rows.map((row) => JSON.stringify(row)).join('\n') + '\n');
  process.exit(0);
};
