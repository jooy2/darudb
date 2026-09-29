/**
 * The notices of the third-party code in the prebuilt addon.
 *
 * The addon links the engine and every crate it and the binding depend on,
 * and one of them, `libmimalloc-sys`, compiles mimalloc's C source in. Their
 * licences ask that their notices go with every copy, the binary included.
 * This collects them from `cargo metadata` and the licence files each crate
 * ships, and writes them into every per-platform package under `npm/`, with
 * this package's own licence, both listed in the package's `files`. The
 * release workflow runs it once `napi create-npm-dirs` has made the packages.
 *
 *     node scripts/notices.mjs          write into every package in npm/
 *     node scripts/notices.mjs --print  print the notices instead
 *
 * A crate is followed for every target the package builds for, so a crate
 * linked on one platform only is listed too, and one no target links, such
 * as a dependency for UEFI, is not. Procedural macros run while the addon
 * compiles and are not in it, so neither they nor what they depend on are
 * listed.
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));
const NOTICES = 'THIRD_PARTY_NOTICES.txt';

/**
 * The licence text of a crate that ships none, from its repository, kept
 * beside this script.
 */
const SHIPPED_APART = {
  napi: 'napi-rs.txt',
  'napi-sys': 'napi-rs.txt'
};

/** Folders of a crate where licence files belong to test data, not to it. */
const NOT_SOURCE = new Set(['tests', 'test', 'benches', 'examples', 'target', '.git']);

/** The names licence files go by. */
const LICENCE_FILE = /^(licen[cs]e|copying|copyright|notice)/i;

/** What a licence file names, when it names one of the licences below. */
const NAMED = {
  MIT: /mit/i,
  'Apache-2.0': /apache/i,
  'Unicode-3.0': /unicode/i
};

/** The crates the addon links, by id, with the licence files of each. */
function linkedCrates() {
  const { napi } = JSON.parse(readFileSync(join(packageDir, 'package.json'), 'utf8'));
  const crates = new Map();

  for (const target of napi.targets) {
    const metadata = JSON.parse(
      execFileSync(
        'cargo',
        ['metadata', '--format-version', '1', '--locked', '--filter-platform', target],
        { cwd: packageDir, encoding: 'utf8', maxBuffer: 1 << 28 }
      )
    );
    const packages = new Map(metadata.packages.map((found) => [found.id, found]));
    const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
    const root = metadata.packages.find((found) => found.name === 'darudb-node');
    const stack = [root.id];
    const seen = new Set();

    while (stack.length > 0) {
      const id = stack.pop();
      const found = packages.get(id);

      if (seen.has(id) || found.targets.some((built) => built.kind.includes('proc-macro'))) {
        continue;
      }

      seen.add(id);

      // The workspace's own packages are under this package's licence.
      if (found.source !== null) {
        crates.set(id, found);
      }

      for (const dep of nodes.get(id).deps) {
        if (dep.dep_kinds.some((kind) => kind.kind === null)) {
          stack.push(dep.pkg);
        }
      }
    }
  }

  return [...crates.values()].sort(
    (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version)
  );
}

/** Every licence file in `dir`, searched three folders deep. */
function licenceFiles(dir, depth = 0) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);

    if (entry.isDirectory()) {
      return depth < 3 && !NOT_SOURCE.has(entry.name) ? licenceFiles(path, depth + 1) : [];
    }

    return LICENCE_FILE.test(entry.name) ? [path] : [];
  });
}

/**
 * The licences a crate is used under: of each alternative its expression
 * offers, MIT where it is one, and the first otherwise.
 */
function licencesUsed(expression) {
  const terms = [];
  let depth = 0;
  let start = 0;

  for (let at = 0; at < expression.length; at++) {
    depth += expression[at] === '(' ? 1 : expression[at] === ')' ? -1 : 0;

    if (depth === 0 && expression.startsWith(' AND ', at)) {
      terms.push(expression.slice(start, at));
      start = at + ' AND '.length;
    }
  }

  terms.push(expression.slice(start));

  return terms.map((term) => {
    const choices = term
      .replace(/[()]/g, '')
      .split(' OR ')
      .map((choice) => choice.trim());

    return choices.includes('MIT') ? 'MIT' : choices[0];
  });
}

/** The licence files of `crate` that the licences it is used under need. */
function filesUsed(crate, files, used) {
  const general = files.filter(
    (file) =>
      !/^(copyright|notice)/i.test(basename(file)) &&
      !Object.values(NAMED).some((pattern) => pattern.test(basename(file)))
  );
  const chosen = new Set(files.filter((file) => /^(copyright|notice)/i.test(basename(file))));

  for (const licence of used) {
    const named = NAMED[licence] ? files.filter((file) => NAMED[licence].test(basename(file))) : [];

    for (const file of named.length > 0 ? named : general) {
      chosen.add(file);
    }
  }

  return [...chosen].sort();
}

/** The notices of every crate the addon links. */
function notices() {
  const sections = linkedCrates().map((crate) => {
    const dir = dirname(crate.manifest_path);
    const used = licencesUsed(crate.license ?? '');
    let texts = filesUsed(crate, licenceFiles(dir), used).map((file) => [
      relative(dir, file),
      readFileSync(file, 'utf8')
    ]);

    if (texts.length === 0 && SHIPPED_APART[crate.name] !== undefined) {
      const file = join(packageDir, 'scripts', 'licenses', SHIPPED_APART[crate.name]);

      texts = [['LICENSE, from the repository', readFileSync(file, 'utf8')]];
    }

    if (texts.length === 0) {
      throw new Error(
        `${crate.name} ${crate.version} ships no licence file: add its text to scripts/licenses and to SHIPPED_APART`
      );
    }

    const lines = [
      '='.repeat(80),
      `${crate.name} ${crate.version}`,
      `Licence used: ${used.join(' AND ')}, of ${crate.license}`,
      ...(crate.repository ? [`Source: ${crate.repository}`] : [])
    ];

    for (const [name, text] of texts) {
      lines.push('', `--- ${name} ---`, '', text.replace(/\r\n/g, '\n').trimEnd());
    }

    return lines.join('\n');
  });

  return [
    'Third-party notices',
    '',
    "The DaruDB addon in this package is compiled from DaruDB's own source,",
    'under the licence in LICENSE, and from the Rust crates below, which are',
    'linked into it. One of them, libmimalloc-sys, compiles the C source of',
    'mimalloc in too. Each is listed with the licence it is used under and the',
    'licence files it ships.',
    '',
    ...sections,
    ''
  ].join('\n');
}

const text = notices();

if (process.argv.includes('--print')) {
  process.stdout.write(text);
} else {
  const npmDir = join(packageDir, 'npm');
  const packages = existsSync(npmDir)
    ? readdirSync(npmDir, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => join(npmDir, entry.name))
        .filter((dir) => existsSync(join(dir, 'package.json')))
    : [];

  if (packages.length === 0) {
    throw new Error('no package in npm/: run `npx napi create-npm-dirs` first');
  }

  for (const dir of packages) {
    const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));

    writeFileSync(join(dir, NOTICES), text);
    copyFileSync(join(packageDir, 'LICENSE'), join(dir, 'LICENSE'));
    manifest.files = [...new Set([...(manifest.files ?? []), 'LICENSE', NOTICES])];
    writeFileSync(join(dir, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  }

  console.log(`Notices written into ${packages.length} packages.`);
}
