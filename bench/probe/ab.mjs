// Probe only: runs passes of harness binaries in turn, each on new files in
// an order that rotates every round, and prints each row's median per side
// with its ratio to the first side's.
//
//   node ab.mjs ROUNDS label=binary:store ...
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [rounds, ...given] = process.argv.slice(2);
const sides = given.map((text) => {
  const [label, rest] = text.split('=');
  const at = rest.lastIndexOf(':');

  return { label, binary: rest.slice(0, at), store: rest.slice(at + 1), rows: new Map() };
});

for (let round = 0; round < Number(rounds); round++) {
  for (let n = 0; n < sides.length; n++) {
    const side = sides[(n + round) % sides.length];
    const dir = mkdtempSync(join(tmpdir(), 'probe-'));
    const pass = spawnSync(side.binary, ['--child', side.store, '--dir', dir], {
      encoding: 'utf8'
    });

    rmSync(dir, { recursive: true, force: true });
    if (pass.status !== 0) throw new Error(`${side.label}: ${pass.stderr}`);

    for (const line of pass.stdout.trim().split('\n')) {
      const row = JSON.parse(line);

      if (!row.ns) continue;
      if (!side.rows.has(row.row)) side.rows.set(row.row, []);
      side.rows.get(row.row).push(row.ns);
    }
  }
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;

  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const spread = (values) => {
  const sorted = [...values].sort((a, b) => a - b);

  return `${Math.round(sorted[0])}..${Math.round(sorted.at(-1))}`;
};

console.log(['row'.padEnd(16), ...sides.map((side) => side.label.padStart(36))].join(''));

for (const row of sides[0].rows.keys()) {
  const base = median(sides[0].rows.get(row));
  const cells = sides.map((side) => {
    const values = side.rows.get(row) ?? [];
    const value = median(values);
    const ratio = side === sides[0] ? '' : ` ${(value / base).toFixed(3)}x`;

    return `${Math.round(value)}${ratio} [${spread(values)}]`.padStart(36);
  });

  console.log([row.padEnd(16), ...cells].join(''));
}
