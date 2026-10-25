// Puts the results bench/run.mjs wrote for some languages into the data the
// documentation's performance page reads, keeping the languages not given as
// they were: the simple suite's under `languages`, the complex one's under
// `complex`.
//
//   node bench/merge.mjs results/rust.json results/rust-complex.json [--into FILE]
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ORDER = ['rust', 'node', 'dart', 'python'];
const args = process.argv.slice(2);
const at = args.indexOf('--into');
const into =
  at < 0
    ? join(dirname(fileURLToPath(import.meta.url)), '../docs/.vitepress/theme/performance.json')
    : resolve(args[at + 1]);
const inputs = args.filter((_, index) => at < 0 || (index !== at && index !== at + 1));
const data = existsSync(into) ? JSON.parse(readFileSync(into, 'utf8')) : { languages: {} };

data.complex ??= {};

for (const input of inputs) {
  const results = JSON.parse(readFileSync(input, 'utf8'));

  if (!ORDER.includes(results.language)) {
    throw new Error(`${input} is not the results of a language this page shows.`);
  }

  if (results.rows.some((row) => !row.agree)) {
    throw new Error(`${input} holds a row whose stores disagree; its times do not compare.`);
  }

  // Results from before the complex suite name no suite.
  data[(results.suite ?? 'simple') === 'complex' ? 'complex' : 'languages'][results.language] =
    results;
}

const ordered = (byLanguage) =>
  Object.fromEntries(
    ORDER.filter((language) => byLanguage[language]).map((language) => [
      language,
      byLanguage[language]
    ])
  );

data.languages = ordered(data.languages);
data.complex = ordered(data.complex);
writeFileSync(into, JSON.stringify(data) + '\n');
console.error(
  `Wrote ${Object.keys(data.languages).join(', ')} and, of the complex suite, ` +
    `${Object.keys(data.complex).join(', ') || 'none'} into ${into}.`
);
