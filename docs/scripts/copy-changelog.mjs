/**
 * Puts every package's `CHANGELOG.md` on the docs site, as one page.
 *
 * A package keeps its changelog beside its manifest, where its registry and a
 * reader browsing that package both expect to find it. Keeping a second copy
 * under `docs/` would be two files that say the same thing until the day one of
 * them does not, so the docs' copy is generated instead — written before
 * VitePress starts and ignored by git.
 *
 * **One page for every package**, each under a heading of its own. The packages
 * version independently and nothing here pretends otherwise: each section is
 * that package's file, whole.
 *
 * What is added on the way in:
 *
 * - **The frontmatter.** The sidebar reads `title` for the label and `order`
 *   for where it sits, and a source file cannot carry either without a
 *   registry rendering it as a stray table at the top.
 * - **The heading and the note at the top**, which belong to the page rather
 *   than to any package, so no file grows a line that only makes sense on a
 *   website.
 * - **Every heading one level down, and the package's name on each version.**
 *   Two packages both have a `## vNext`, and the two would land on one anchor,
 *   with the loser silently becoming `-1`. Naming the package in the heading
 *   makes the anchor VitePress derives unique on its own, and says the useful
 *   thing on a page holding several histories besides.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const docsDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(docsDir, '..');

/** One entry per locale served by the docs. Keep in step with `supportLocales`. */
const TITLES = {
  en: 'Changelog',
  ko: '변경 기록'
};

/**
 * The line under the page's heading, in each locale. It says that the entries
 * are English wherever the page is not.
 */
const NOTES = {
  en: 'Each package versions on its own and keeps its own history. This page shows all of them, newest first within each.',
  ko: '패키지마다 버전을 따로 매기고 변경 기록도 따로 둡니다. 이 페이지는 모든 패키지의 기록을 최신순으로 보여 줍니다. 변경 기록은 패키지와 함께 관리하며 영어로 작성하므로, 아래 항목은 원문 그대로입니다.'
};

/** Which package's changelog is which section of the page. */
const PACKAGES = [
  {
    label: 'Rust',
    heading: { en: 'Rust crate `darudb`', ko: 'Rust 크레이트 `darudb`' },
    source: 'crates/darudb/CHANGELOG.md'
  },
  {
    label: 'Rust derive',
    heading: { en: 'Rust crate `darudb-derive`', ko: 'Rust 크레이트 `darudb-derive`' },
    source: 'crates/darudb-derive/CHANGELOG.md'
  },
  {
    label: 'Node.js',
    heading: { en: 'Node.js package `darudb`', ko: 'Node.js 패키지 `darudb`' },
    source: 'packages/node/CHANGELOG.md'
  }
];

/**
 * One package's changelog, as its section of the page.
 *
 * The file's own `# Changelog` and the blockquote under it go: the page has a
 * heading already, and the note about versioning independently is said once
 * above rather than once per package.
 */
const sectionOf = ({ label, heading, source }, locale) => {
  const changelog = readFileSync(resolve(repoRoot, source), 'utf8')
    .replace(/^# Changelog\n+> [^\n]*\n+/, '')
    .replace(/^### (.+)$/gm, '#### $1')
    .replace(/^## (.+)$/gm, `### ${label} $1`)
    .trimEnd();

  return [`## ${heading[locale] ?? heading.en}`, '', changelog].join('\n');
};

for (const [locale, title] of Object.entries(TITLES)) {
  const target = resolve(docsDir, locale, 'changelog.md');

  mkdirSync(dirname(target), { recursive: true });

  writeFileSync(
    target,
    [
      '---',
      `title: ${title}`,
      'order: 1',
      'editLink: false',
      'outline: [2, 3]',
      '---',
      '',
      `# ${title}`,
      '',
      NOTES[locale] ?? NOTES.en,
      '',
      PACKAGES.map((item) => sectionOf(item, locale)).join('\n\n'),
      ''
    ].join('\n'),
    'utf8'
  );
}
