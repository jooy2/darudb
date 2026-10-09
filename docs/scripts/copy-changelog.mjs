/**
 * Puts every package's `CHANGELOG.md` on the docs site, one page for each
 * language the packages are for.
 *
 * A package keeps its changelog beside its manifest, where its registry and a
 * reader browsing that package both expect to find it. Keeping a second copy
 * under `docs/` would be two files that say the same thing until the day one of
 * them does not, so the docs' copy is generated instead — written before
 * VitePress starts and ignored by git.
 *
 * **One page for each language**, `changelog/rust.md` and so on, which is a
 * per-language section like the API's: the reader's language decides which
 * page the sidebar lists, the language switch moves between them, and
 * `changelog/index.md`, where the navbar's link and older links lead, sends a
 * reader on to theirs. A language with two packages, the Rust crate and its
 * derive macros, or the Dart package and its generator, has both on its page,
 * each under a heading of its own. The packages version independently and
 * nothing here pretends otherwise: each section is that package's file, whole.
 *
 * What is added on the way in:
 *
 * - **The frontmatter.** The sidebar reads `title` and `order`, and a source
 *   file cannot carry either without a registry rendering it as a stray table
 *   at the top.
 * - **The heading and the note at the top**, which belong to the page rather
 *   than to any package, so no file grows a line that only makes sense on a
 *   website.
 * - **Every heading one level down, and the package's name on each version.**
 *   Two packages on one page both have a `## v1.1.0`, and the two would land on
 *   one anchor, with the loser silently becoming `-1`. Naming the package in
 *   the heading makes the anchor VitePress derives unique on its own.
 */
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const docsDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(docsDir, '..');

/** The locales served by the docs. Keep in step with `supportLocales`. */
const LOCALES = ['en', 'ko'];

/** The overview's title, and the word each language's page title ends with. */
const TITLES = {
  en: { index: 'Changelog', page: (language) => `${language} changelog` },
  ko: { index: '변경 기록', page: (language) => `${language} 변경 기록` }
};

/**
 * The line under a language's heading. It names the packages on the page and
 * says, on the Korean site, that the entries are English.
 */
const NOTES = {
  en: (packages) =>
    `The history of the ${packages}, newest first. Every package versions on its own and keeps its own history; choose another language in the sidebar for its packages' history.`,
  ko: (packages) =>
    `${packages}의 변경 기록이며, 최신 기록이 위에 옵니다. 패키지마다 버전을 따로 매기고 기록도 따로 둡니다. 다른 언어 패키지의 기록은 사이드바에서 언어를 바꾸면 볼 수 있습니다. 변경 기록은 패키지와 함께 관리하며 영어로 작성하므로, 아래 항목은 원문 그대로입니다.`
};

/** The overview's text, for a reader whose browser did not send them on. */
const INDEX_NOTES = {
  en: 'Each package keeps its own changelog, and each language has a page with its packages:',
  ko: '패키지마다 변경 기록을 따로 두며, 언어마다 그 언어의 패키지 기록을 한 페이지에 모았습니다.'
};

/**
 * Each language's page, in the order the languages are named everywhere, and
 * the changelogs on it. The ids are the language switch's (`languages.ts`).
 */
const LANGUAGES = [
  {
    id: 'rust',
    label: 'Rust',
    summary: {
      en: 'Rust crates `darudb` and `darudb-derive`',
      ko: 'Rust 크레이트 `darudb`와 `darudb-derive`'
    },
    packages: [
      {
        name: 'darudb',
        heading: { en: 'Rust crate `darudb`', ko: 'Rust 크레이트 `darudb`' },
        source: 'crates/darudb/CHANGELOG.md'
      },
      {
        name: 'darudb-derive',
        heading: { en: 'Rust crate `darudb-derive`', ko: 'Rust 크레이트 `darudb-derive`' },
        source: 'crates/darudb-derive/CHANGELOG.md'
      }
    ]
  },
  {
    id: 'node',
    label: 'Node.js',
    summary: { en: 'Node.js package `darudb`', ko: 'Node.js 패키지 `darudb`' },
    packages: [
      {
        name: 'darudb',
        heading: { en: 'Node.js package `darudb`', ko: 'Node.js 패키지 `darudb`' },
        source: 'packages/node/CHANGELOG.md'
      }
    ]
  },
  {
    id: 'dart',
    label: 'Dart',
    summary: {
      en: 'Dart packages `darudb` and `darudb_generator`',
      ko: 'Dart 패키지 `darudb`와 `darudb_generator`'
    },
    packages: [
      {
        name: 'darudb',
        heading: { en: 'Dart package `darudb`', ko: 'Dart 패키지 `darudb`' },
        source: 'packages/dart/darudb/CHANGELOG.md'
      },
      {
        name: 'darudb_generator',
        heading: { en: 'Dart package `darudb_generator`', ko: 'Dart 패키지 `darudb_generator`' },
        source: 'packages/dart/darudb_generator/CHANGELOG.md'
      }
    ]
  },
  {
    id: 'python',
    label: 'Python',
    summary: { en: 'Python package `darudb`', ko: 'Python 패키지 `darudb`' },
    packages: [
      {
        name: 'darudb',
        heading: { en: 'Python package `darudb`', ko: 'Python 패키지 `darudb`' },
        source: 'packages/python/CHANGELOG.md'
      }
    ]
  }
];

/**
 * One package's changelog, as its section of a page.
 *
 * The file's own `# Changelog` and the blockquote under it go: the page has a
 * heading already, and the note about versioning independently is said once
 * above rather than once per package.
 */
const sectionOf = ({ name, heading, source }, locale) => {
  const changelog = readFileSync(resolve(repoRoot, source), 'utf8')
    .replace(/^# Changelog\n+> [^\n]*\n+/, '')
    .replace(/^### (.+)$/gm, '#### $1')
    .replace(/^## (.+)$/gm, `### ${name} $1`)
    .trimEnd();

  return [`## ${heading[locale] ?? heading.en}`, '', changelog].join('\n');
};

const frontmatter = (fields) => [
  '---',
  ...Object.entries(fields).map(([key, value]) => `${key}: ${value}`),
  '---'
];

for (const locale of LOCALES) {
  const folder = resolve(docsDir, locale, 'changelog');

  // The single page this script used to write, which would shadow the folder.
  rmSync(resolve(docsDir, locale, 'changelog.md'), { force: true });
  mkdirSync(folder, { recursive: true });

  for (const language of LANGUAGES) {
    const title = TITLES[locale].page(language.label);

    writeFileSync(
      resolve(folder, `${language.id}.md`),
      [
        ...frontmatter({ title, order: 1, editLink: false, outline: '[2, 3]' }),
        '',
        `# ${title}`,
        '',
        NOTES[locale](language.summary[locale] ?? language.summary.en),
        '',
        language.packages.map((item) => sectionOf(item, locale)).join('\n\n'),
        ''
      ].join('\n'),
      'utf8'
    );
  }

  // The overview a link to `/changelog` lands on. The script in `<head>` and
  // the router send a reader with scripts on to their language's page, so
  // this is what a crawler or a reader without scripts sees.
  writeFileSync(
    resolve(folder, 'index.md'),
    [
      ...frontmatter({ title: TITLES[locale].index, editLink: false }),
      '',
      `# ${TITLES[locale].index}`,
      '',
      INDEX_NOTES[locale],
      '',
      ...LANGUAGES.map(
        (language) => `- [${language.label}](./${language.id}.md): ${language.summary[locale]}`
      ),
      ''
    ].join('\n'),
    'utf8'
  );
}
