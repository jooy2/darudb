// Runs the benchmark of bench/README.md for one language and writes its
// results as JSON: every store of that language, each pass in a child
// process of its own on new files, the stores taking turns in an order that
// moves by one every run.
//
//   node bench/run.mjs <rust|node|dart|python> [--runs 5] [--only daru,sqlite] [--dir DIR] [--out FILE]
//
// A cell is the median of the runs, with the fastest and the slowest beside
// it. A row whose results differ between stores, or between runs, fails the
// run: the stores did not do the same work, and their times do not compare.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const bench = dirname(fileURLToPath(import.meta.url));
const root = resolve(bench, '..');
const args = process.argv.slice(2);
const language = args[0];
const option = (name, fallback) => {
  const at = args.indexOf(name);

  return at < 0 ? fallback : args[at + 1];
};
const runs = Number(option('--runs', '5'));
const only = option('--only', null)?.split(',');
const base = option('--dir', os.tmpdir());
const out = option('--out', null);

/** How each language lists its stores and runs one pass of one. */
const LANGUAGES = {
  rust: () => {
    const binary = join(
      bench,
      'rust/target/release',
      process.platform === 'win32' ? 'darudb-bench.exe' : 'darudb-bench'
    );

    return {
      stores: [binary, '--stores'],
      child: (store, dir) => [binary, '--child', store, '--dir', dir]
    };
  },
  node: () => {
    const daru = resolve(option('--daru', join(root, 'packages/node')));

    return {
      stores: [process.execPath, join(bench, 'node/stores.mjs'), daru],
      child: (store, dir) => [process.execPath, join(bench, `node/${store}.mjs`), dir, daru]
    };
  },
  dart: () => {
    // The harness compiled ahead of time with `dart build cli`, which is how
    // a Flutter app runs, in the folder named after this platform.
    const target = `${process.platform === 'darwin' ? 'macos' : process.platform === 'win32' ? 'windows' : 'linux'}_${process.arch}`;
    const binary =
      process.env.BENCH_DART ??
      join(
        bench,
        `dart/build/cli/${target}/bundle/bin/bench${process.platform === 'win32' ? '.exe' : ''}`
      );

    return {
      stores: [binary, '--stores', '--lock', join(bench, 'dart/pubspec.lock')],
      child: (store, dir) => [binary, '--child', store, '--dir', dir]
    };
  },
  python: () => {
    const python = process.env.BENCH_PYTHON ?? 'python3';
    const script = join(bench, 'python/bench.py');

    return {
      stores: [python, script, '--stores'],
      child: (store, dir) => [python, script, '--child', store, '--dir', dir]
    };
  }
};

if (!(language in LANGUAGES)) {
  console.error(`The first argument names a language: ${Object.keys(LANGUAGES).join(', ')}.`);
  process.exit(2);
}

const commands = LANGUAGES[language]();

const run = (command) => {
  const child = spawnSync(command[0], command.slice(1), {
    encoding: 'utf8',
    maxBuffer: 1 << 26
  });

  if (child.status !== 0) {
    console.error(`${command.join(' ')} failed:\n${child.stderr ?? child.error}`);
    process.exit(1);
  }

  return child.stdout;
};

const stores = JSON.parse(run(commands.stores)).filter((store) => !only || only.includes(store.id));
const seen = Object.fromEntries(stores.map((store) => [store.id, new Map()]));
const problems = [];

for (let pass = 0; pass < runs; pass++) {
  for (let turn = 0; turn < stores.length; turn++) {
    const { id } = stores[(pass + turn) % stores.length];
    const dir = mkdtempSync(join(base, `darudb-bench-${language}-${id}-`));
    const lines = run(commands.child(id, dir)).trim().split('\n');

    rmSync(dir, { recursive: true, force: true });

    for (const line of lines) {
      const { row, ns, count, hash } = JSON.parse(line);
      const kept = seen[id].get(row);

      // A time of null is a row the store has no way to do, such as a
      // deferred commit in a store whose every commit syncs.
      if (ns === null) {
        if (!kept) seen[id].set(row, null);
      } else if (!kept) {
        seen[id].set(row, { ns: [ns], count, hash });
      } else {
        if (kept.count !== count || kept.hash !== hash) {
          problems.push(`${id} gave other results for ${row} in run ${pass + 1}`);
        }
        kept.ns.push(ns);
      }
    }

    console.error(`${language}: run ${pass + 1} of ${runs}, ${id} done`);
  }
}

const median = (sorted) => {
  const middle = Math.floor(sorted.length / 2);

  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

const first = seen[stores[0].id];
const rows = [...first.keys()].map((row) => {
  // The first store that does the row is what the others are checked against.
  const reference = stores.map(({ id }) => seen[id].get(row)).find(Boolean) ?? {
    count: 0,
    hash: 0
  };
  const agree = stores.every(({ id }) => {
    const cell = seen[id].get(row);

    return (
      cell === null ||
      (cell !== undefined && cell.count === reference.count && cell.hash === reference.hash)
    );
  });

  if (!agree) {
    problems.push(`the stores' results differ in ${row}`);
  }

  return {
    id: row,
    count: reference.count,
    agree,
    results: Object.fromEntries(
      stores.map(({ id }) => {
        const cell = seen[id].get(row);

        if (!cell) return [id, null];

        const values = [...cell.ns].sort((a, b) => a - b);

        return [id, { median: median(values), min: values[0], max: values.at(-1) }];
      })
    )
  };
});

const cpus = os.cpus();
const results = {
  language,
  generated: new Date().toISOString().replace(/\.\d+Z$/, 'Z'),
  runs,
  machine: {
    os: `${os.type()} ${os.release()}`,
    cpu: cpus[0]?.model.trim() ?? '',
    cores: cpus.length,
    memory: Math.round(os.totalmem() / 2 ** 30)
  },
  stores,
  rows
};

if (out) {
  writeFileSync(out, JSON.stringify(results, null, 2) + '\n');
} else {
  process.stdout.write(JSON.stringify(results, null, 2) + '\n');
}

const format = (ns) =>
  ns === 0
    ? '-'
    : ns >= 1e6
      ? `${(ns / 1e6).toFixed(2)} ms`
      : ns >= 1e3
        ? `${(ns / 1e3).toFixed(2)} us`
        : `${Math.round(ns)} ns`;

console.error(''.padEnd(18) + stores.map(({ id }) => id.padStart(12)).join(''));

for (const row of rows) {
  console.error(
    row.id.padEnd(18) +
      stores.map(({ id }) => format(row.results[id]?.median ?? 0).padStart(12)).join('')
  );
}

for (const problem of problems) {
  console.error(`DIFFERS: ${problem}`);
}

process.exit(problems.length ? 1 : 0);
