import container from 'markdown-it-container';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { withSidebar } from 'vitepress-sidebar';
import packageJson from '../../packages/node/package.json' with { type: 'json' };
import {
  defineConfig,
  HeadConfig,
  MarkdownRenderer,
  SiteData,
  TransformContext,
  UserConfig
} from 'vitepress';
import { withI18n } from 'vitepress-i18n';
import type { VitePressI18nOptions } from 'vitepress-i18n/types';
import type { VitePressSidebarOptions } from 'vitepress-sidebar/types';
import {
  LANGUAGE_IDS,
  languageHeadScript,
  languageOfRoute,
  languagesOf,
  type LanguageId,
  type LanguagePages,
  type Registry
} from './languages';
import { languageIcons } from './icons';
import { summaryFromSource } from './pages';

const vitePressDir = dirname(fileURLToPath(import.meta.url));
/** `docs/`, which is where the locale folders live and what VitePress serves. */
const srcDir = resolve(vitePressDir, '..');

const defaultLocale: string = 'en';
const supportLocales: string[] = [defaultLocale, 'ko'];
const editLinkPattern = `${packageJson.repository.url}/edit/main/docs/:path`;

/*
 * The site's host and the repository, read off the npm package rather than
 * written again. The package is where a registry already looks for both, so it
 * is the one place to change when either moves.
 */
const siteUrl = packageJson.homepage.replace(/\/+$/, '');
const repoUrl = packageJson.repository.url.replace(/\.git$/, '');
const npmUrl = `https://www.npmjs.com/package/${packageJson.name}`;
const cratesUrl = 'https://crates.io/crates/darudb';
const pypiUrl = 'https://pypi.org/project/darudb/';
const pubUrl = 'https://pub.dev/packages/darudb';

/**
 * Every package a reader installs, by the registry it is published on, for the
 * package menu in the navbar. In the order the languages are named everywhere.
 */
const registries: Registry[] = [
  {
    language: 'rust',
    name: 'crates.io',
    packages: [
      { name: 'darudb', link: cratesUrl },
      { name: 'darudb-derive', link: 'https://crates.io/crates/darudb-derive' }
    ]
  },
  { language: 'node', name: 'npm', packages: [{ name: packageJson.name, link: npmUrl }] },
  {
    language: 'dart',
    name: 'pub.dev',
    packages: [
      { name: 'darudb', link: pubUrl },
      { name: 'darudb_generator', link: 'https://pub.dev/packages/darudb_generator' }
    ]
  },
  { language: 'python', name: 'PyPI', packages: [{ name: 'darudb', link: pypiUrl }] }
];

/**
 * What the site is about, in the default locale. The npm package has a
 * description of its own, but that one is about the Node.js binding, and this
 * site is about every package.
 */
const SITE_DESCRIPTION =
  "An embedded database that keeps an application's data in one local file, for Rust, Node.js, Dart and Python. One engine written in Rust, with encryption, crash safety and several processes on one file built in.";

/** `/` for whichever locale is the default, `/{lang}/` for every other one. */
const localeBase = (lang: string) => (lang === defaultLocale ? '/' : `/${lang}/`);

const commonSidebarConfig: VitePressSidebarOptions = {
  collapsed: false,
  // Off, because every page here declares its own title, and a reference page
  // is named after the thing it documents. Capitalising would print
  // `openOptions` as "OpenOptions" — a name that is not the name.
  capitalizeFirst: false,
  useTitleFromFileHeading: true,
  useTitleFromFrontmatter: true,
  useFolderTitleFromIndexFile: true,
  useFolderLinkFromIndexFile: true,
  frontmatterOrderDefaultValue: 9,
  sortMenusByFrontmatterOrder: true
};

/**
 * The sidebar's groups, in each locale.
 *
 * Named here rather than by the folders' own titles, because `guide/` and
 * `engine/` have no `index.md` to take a heading from, and a group called
 * "guide" over Korean pages is not a heading. `changelog` is the entry each
 * language's changelog shows under, since the sidebar lists the reader's alone
 * and the language in its title would say nothing the switch above does not.
 */
const groupLabels: Record<
  string,
  {
    guide: string;
    engine: string;
    api: string;
    types: string;
    more: string;
    changelog: string;
    migration: string;
  }
> = {
  en: {
    guide: 'Guide',
    engine: 'Engine',
    api: 'API',
    types: 'Types',
    more: 'Discover more',
    changelog: 'Changelog',
    migration: 'Migration'
  },
  ko: {
    guide: '가이드',
    engine: '엔진',
    api: 'API',
    types: '타입',
    more: '더 알아보기',
    changelog: '변경 기록',
    migration: '다른 DB에서 옮기기'
  }
};

const vitePressSidebarConfig = supportLocales.map((lang) => ({
  ...commonSidebarConfig,
  // Relative to the working directory, which is this `docs/` folder —
  // `vitepress-sidebar` joins it onto `process.cwd()`.
  documentRootPath: `/${lang}`,
  resolvePath: localeBase(lang),
  ...(defaultLocale === lang ? {} : { basePath: localeBase(lang) })
}));

/** The same destinations in every locale, prefixed with its base. */
const navFor = (lang: string, labels: [string, string, string, string, string]) => [
  { text: labels[0], link: `${localeBase(lang)}guide/introduction`, activeMatch: '/guide/' },
  { text: labels[1], link: `${localeBase(lang)}engine/architecture`, activeMatch: '/engine/' },
  { text: labels[2], link: `${localeBase(lang)}api/`, activeMatch: '/api/' },
  { text: labels[3], link: `${localeBase(lang)}types/`, activeMatch: '/types/' },
  // The overview, which sends a reader on to their language's changelog.
  { text: labels[4], link: `${localeBase(lang)}changelog/`, activeMatch: '/changelog/' }
];

const vitePressI18nConfig: VitePressI18nOptions = {
  locales: supportLocales,
  rootLocale: defaultLocale,
  searchProvider: 'local',
  description: {
    en: SITE_DESCRIPTION,
    ko: '애플리케이션의 데이터를 로컬 파일 하나에 담는 임베디드 데이터베이스입니다. Rust로 작성한 엔진 하나를 Rust와 Node.js, Dart, Python에서 함께 쓰며, 암호화와 크래시 안전성, 여러 프로세스의 동시 접근을 엔진에 담았습니다.'
  },
  themeConfig: {
    en: { nav: navFor('en', ['Guide', 'Engine', 'API', 'Types', 'Changelog']) },
    ko: { nav: navFor('ko', ['가이드', '엔진', 'API', '타입', '변경 기록']) }
  }
};

/* ---------------------------------------------------------------------------
 * Search engines
 *
 * Two things a documentation site gets wrong by default, and both of them are
 * per page rather than per site:
 *
 * - **Every page ships the same description.** VitePress falls back to the
 *   site's own whenever a page declares none, so every page carries one
 *   sentence between them and not one of them says what it is about. The lede
 *   a page already opens with is written to be exactly this, so it is read out
 *   of the source instead.
 * - **Nothing says the two locales are the same page.** Without `hreflang` a
 *   crawler has no reason to connect `/guide/introduction` to its Korean
 *   counterpart, and treats them as two documents competing for one query.
 * ------------------------------------------------------------------------- */

/**
 * The BCP-47 tag the site itself declares for a locale — `en` → `en-US`.
 *
 * Read back off the resolved config rather than written out again, because
 * VitePress's own sitemap already emits `hreflang` from exactly these values.
 */
function langTagOf(siteData: SiteData, lang: string): string {
  return siteData.locales[lang === defaultLocale ? 'root' : lang]?.lang ?? lang;
}

/** `en/guide/introduction.md` → `/guide/introduction`. */
function pathOf(filePath: string): string {
  const [lang, ...rest] = filePath.split('/');
  const page = rest
    .join('/')
    .replace(/(^|\/)index\.md$/, '$1')
    .replace(/\.md$/, '');

  return `${localeBase(lang)}${page}`;
}

/** Everything below the locale folder — the part two locales have in common. */
function pageOf(filePath: string): string {
  return filePath.split('/').slice(1).join('/');
}

/** A page's own one-line summary, read from its source. See `pages.ts`. */
function summaryOf(filePath: string): string | undefined {
  const file = resolve(srcDir, filePath);

  return existsSync(file) ? summaryFromSource(readFileSync(file, 'utf8')) : undefined;
}

/** One field out of a page's frontmatter, if the page declares it. */
function frontmatterOf(filePath: string, field: string): string | undefined {
  const file = resolve(srcDir, filePath);

  if (!existsSync(file)) {
    return undefined;
  }

  const source = readFileSync(file, 'utf8');
  const block = /^---\r?\n([\s\S]*?)\r?\n---/.exec(source)?.[1];

  return block ? new RegExp(`^${field}:[ \\t]*(.+?)[ \\t]*$`, 'm').exec(block)?.[1] : undefined;
}

/** Every Markdown page under a folder, in the order a reader meets them. */
function pagesUnder(folder: string): string[] {
  const found: string[] = [];

  const walk = (at: string) => {
    const entries = readdirSync(resolve(srcDir, at), { withFileTypes: true });

    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const path = `${at}/${entry.name}`;

      if (entry.isDirectory()) {
        walk(path);
      } else if (entry.name.endsWith('.md')) {
        found.push(path);
      }
    }
  };

  walk(folder);

  return found;
}

/* ---------------------------------------------------------------------------
 * Programming languages
 *
 * Which pages are written for which language. A page of a per-language
 * section says it with its folder (`api/rust/`); any other page that is not
 * for every language says it with `languages` in its frontmatter. The script
 * in `<head>`, the sidebar and the switch all need the answer for pages they
 * are not rendering, so it is collected here, once, from the files.
 * ------------------------------------------------------------------------- */

/** `/guide/async` for `en/guide/async.md` and for `ko/guide/async.md` alike. */
function routeOfFile(filePath: string): string {
  const page = pageOf(filePath)
    .replace(/(^|\/)index\.md$/, '$1')
    .replace(/\.md$/, '')
    .replace(/\/$/, '');

  return `/${page}`;
}

/** `[rust, node]`, `rust` or nothing, as a frontmatter value. */
function listOf(value: string | undefined): string[] {
  return (value ?? '')
    .replace(/^\[|\]$/g, '')
    .split(',')
    .map((item) => item.trim().replace(/^['"]|['"]$/g, ''))
    .filter(Boolean);
}

/**
 * Reads every page's languages and counterpart, and checks them.
 *
 * A typo in `languages` would hide a page from everyone, a counterpart that
 * does not exist would send a reader to a 404, and a translation that names
 * other languages than its original would make the two locales' sidebars
 * disagree, which nobody reading one locale would notice. The build refuses
 * all three; the dev server only warns, so that a page half written still
 * loads.
 */
function collectLanguagePages(): LanguagePages {
  const pages: LanguagePages = { only: {}, reference: [], counterparts: {} };
  const routes = new Set<string>();
  const problems: string[] = [];

  for (const filePath of pagesUnder(defaultLocale)) {
    const route = routeOfFile(filePath);

    routes.add(route);

    if (languageOfRoute(route)) {
      pages.reference.push(route);
      continue;
    }

    const languages = listOf(frontmatterOf(filePath, 'languages'));
    const unknown = languages.filter((id) => !(LANGUAGE_IDS as string[]).includes(id));

    if (unknown.length) {
      problems.push(`${filePath} names unknown languages: ${unknown.join(', ')}.`);
    } else if (languages.length) {
      pages.only[route] = languages as LanguageId[];
    }
  }

  for (const filePath of pagesUnder(defaultLocale)) {
    const route = routeOfFile(filePath);

    // One counterpart, or a list of them, one for each other language.
    for (const counterpart of listOf(frontmatterOf(filePath, 'counterpart'))) {
      if (!routes.has(counterpart)) {
        problems.push(`${filePath} has a counterpart that does not exist: ${counterpart}.`);
        continue;
      }

      for (const [from, to] of [
        [route, counterpart],
        [counterpart, route]
      ]) {
        const known = (pages.counterparts[from] ??= []);

        if (!known.includes(to)) {
          known.push(to);
        }
      }
    }
  }

  for (const locale of supportLocales.filter((lang) => lang !== defaultLocale)) {
    for (const filePath of pagesUnder(locale)) {
      const route = routeOfFile(filePath);

      if (languageOfRoute(route) || !routes.has(route)) {
        continue;
      }

      const translated = listOf(frontmatterOf(filePath, 'languages')).join(', ');
      const original = (pages.only[route] ?? []).join(', ');

      if (translated !== original) {
        problems.push(
          `${filePath} is for ${translated || 'every language'}, ` +
            `where ${defaultLocale}${route}.md is for ${original || 'every language'}.`
        );
      }
    }
  }

  if (problems.length) {
    const report = ['The pages below name languages that do not add up:', ...problems].join('\n  ');

    // `vitepress build` against `vitepress dev`: nothing that runs while the
    // config loads knows which one it is in any other way.
    if (process.argv.includes('build')) {
      throw new Error(report);
    }

    console.warn(report);
  }

  return pages;
}

const languagePages = collectLanguagePages();

/** The locales other than the default, which are the routes' prefixes. */
const localePrefixes = supportLocales.filter((lang) => lang !== defaultLocale);

/** The pages written for one language, which the script in `<head>` switches to. */
const singleLanguagePages = Object.fromEntries(
  Object.entries(languagePages.only)
    .filter(([, languages]) => languages.length === 1)
    .map(([route, languages]) => [route, languages[0]])
);

/**
 * Sidebar rules for the shared pages that are written for some languages only.
 *
 * The per-language sections are hidden by their folders in `languages.css`.
 * These pages have no folder to go by, so each gets a rule naming its link in
 * every locale, written into `<head>` so that it applies before the first
 * paint rather than after the app has started.
 */
function sidebarStyle(): string {
  const rules = LANGUAGE_IDS.map((id) => {
    const selectors = Object.entries(languagePages.only)
      .filter(([, languages]) => !languages.includes(id))
      .flatMap(([route]) =>
        supportLocales.map(
          (lang) =>
            `html[data-code-lang='${id}'] .VPSidebarItem:has(> .item > .link[href='${localeBase(lang)}${route.slice(1)}'])`
        )
      );

    return selectors.length ? `${selectors.join(',')}{display:none}` : '';
  });

  return rules.join('');
}

/**
 * `llms.txt`, which is this site's own map written for something reading it
 * rather than browsing it.
 *
 * The default locale only. The two are the same pages, an agent asking what
 * this database is does not need both, and the sitemap's `hreflang` is where
 * the other one is announced.
 *
 * Built from the pages that are actually there, for the same reason
 * `robots.txt` is: a hand-written list of links is a list that goes stale, and
 * a stale one is worse than no file at all.
 */
function llmsTxt(): string {
  const lines = [
    '# DaruDB',
    '',
    `> ${SITE_DESCRIPTION}`,
    '',
    'One engine, written in Rust, shipped to Rust as the crate `darudb`, to Node.js as the npm',
    'package `darudb`, to Dart as the package `darudb`, and to Python as the PyPI package `darudb`. Every language',
    'reads and writes the same file the same way, because every rule about the file lives in the engine.',
    ''
  ];

  const sections = [
    ['Guide', 'guide'],
    ['Engine', 'engine'],
    ['Rust API', 'api/rust'],
    ['Rust types', 'types/rust'],
    ['Node.js API', 'api/node'],
    ['Node.js types', 'types/node'],
    ['Dart API', 'api/dart'],
    ['Dart types', 'types/dart'],
    ['Python API', 'api/python'],
    ['Python types', 'types/python'],
    ['Migration', 'migration']
  ];

  for (const [name, section] of sections) {
    const folder = `${defaultLocale}/${section}`;

    if (!existsSync(resolve(srcDir, folder))) {
      continue;
    }

    lines.push(`## ${name}`, '');

    // In the order the sidebar puts them in: the folders in turn, and inside
    // each the `order` its pages carry. A page with no `order` goes last,
    // where `vitepress-sidebar` puts it, and a folder's own `index.md` first.
    const rank = (filePath: string) =>
      filePath.endsWith('/index.md') ? -1 : Number(frontmatterOf(filePath, 'order') ?? 9);
    const pages = pagesUnder(folder).sort(
      (a, b) => dirname(a).localeCompare(dirname(b)) || rank(a) - rank(b) || a.localeCompare(b)
    );

    for (const filePath of pages) {
      const title = frontmatterOf(filePath, 'title');
      // The page's own description where it wrote one, and its opening
      // sentence where it did not — the same two the `<meta>` comes from.
      const summary = frontmatterOf(filePath, 'description') ?? summaryOf(filePath);

      if (title && summary) {
        lines.push(`- [${title}](${siteUrl}${pathOf(filePath)}): ${summary}`);
      }
    }

    lines.push('');
  }

  // What a reader may skip when its context is short, which is the whole of
  // what this heading is reserved for.
  const changelogs = [
    ['Rust', 'rust', 'the crates `darudb` and `darudb-derive`'],
    ['Node.js', 'node', 'the npm package `darudb`'],
    ['Dart', 'dart', 'the packages `darudb` and `darudb_generator`'],
    ['Python', 'python', 'the PyPI package `darudb`']
  ].map(
    ([name, id, packages]) =>
      `- [${name} changelog](${siteUrl}${pathOf(`${defaultLocale}/changelog/${id}.md`)}): the changes to ${packages}, newest first`
  );

  lines.push(
    '## Optional',
    '',
    ...changelogs,
    `- [Repository](${repoUrl}): the engine, the bindings, and how to build and test each`,
    ''
  );

  return lines.join('\n');
}

/** The locales that actually have this page — a mirror is not a guarantee. */
function localesWith(filePath: string): string[] {
  const page = pageOf(filePath);

  return supportLocales.filter((lang) => existsSync(resolve(srcDir, lang, page)));
}

/** What the project is, for the one page in each locale that is about it. */
function structuredData(description: string, url: string) {
  return {
    '@context': 'https://schema.org',
    '@type': 'SoftwareSourceCode',
    name: 'DaruDB',
    description,
    url,
    codeRepository: repoUrl,
    programmingLanguage: ['Rust', 'TypeScript'],
    runtimePlatform: ['Node.js'],
    license: 'https://opensource.org/licenses/MIT',
    author: { '@type': 'Organization', name: 'CDGet', url: 'https://cdget.com' },
    sameAs: [repoUrl, npmUrl, cratesUrl, pubUrl, pypiUrl]
  };
}

/**
 * The half of the metadata that is different on every page.
 *
 * Only runs at build time — `transformPageData` is what the dev server sees —
 * so the tags below are checked by reading a built page, not the preview.
 */
function transformHead({ pageData, siteData, title, description }: TransformContext): HeadConfig[] {
  const { filePath } = pageData;

  // A dynamic route, or the built-in 404: no source file, so no canonical URL
  // and nothing to point an alternate at.
  if (!filePath) {
    return [];
  }

  const lang = filePath.split('/')[0];
  const url = `${siteUrl}${pathOf(filePath)}`;
  const translations = localesWith(filePath);

  // Open Graph writes a BCP-47 tag with an underscore in it, and nothing else.
  const ogLocale = (of: string) => langTagOf(siteData, of).replace('-', '_');

  const head: HeadConfig[] = [
    ['link', { rel: 'canonical', href: url }],
    ['meta', { property: 'og:url', content: url }],
    ['meta', { property: 'og:title', content: title }],
    ['meta', { property: 'og:description', content: description }],
    ['meta', { property: 'og:locale', content: ogLocale(lang) }],
    ['meta', { name: 'twitter:title', content: title }],
    ['meta', { name: 'twitter:description', content: description }]
  ];

  for (const other of translations) {
    head.push([
      'link',
      {
        rel: 'alternate',
        hreflang: langTagOf(siteData, other),
        href: `${siteUrl}${pathOf(`${other}/${pageOf(filePath)}`)}`
      }
    ]);

    if (other !== lang) {
      head.push(['meta', { property: 'og:locale:alternate', content: ogLocale(other) }]);
    }
  }

  // Which one a crawler should serve to a reader it cannot place. The default
  // locale is the one that is served from `/`.
  if (translations.includes(defaultLocale)) {
    head.push([
      'link',
      {
        rel: 'alternate',
        hreflang: 'x-default',
        href: `${siteUrl}${pathOf(`${defaultLocale}/${pageOf(filePath)}`)}`
      }
    ]);
  }

  if (pageData.frontmatter.layout === 'home') {
    head.push([
      'script',
      { type: 'application/ld+json' },
      JSON.stringify(structuredData(description, url))
    ]);
  }

  return head;
}

/**
 * A heading's anchor, with Korean kept whole.
 *
 * The default slug decomposes the heading (NFKD) to strip accents from Latin
 * letters, which also splits every Korean syllable into its letters. The id
 * of `## 파일 백업하기` then holds those letters, while a link to
 * `#파일-백업하기` is written, like all Korean text, with whole syllables:
 * the two never compare equal, and the link scrolls nowhere. These are the
 * default's steps, with the result composed again (NFC) at the end.
 */
function slugifyHeading(text: string): string {
  return text
    .normalize('NFKD')
    .replace(/[\u0300-\u036F]/g, '')
    .replace(/[\u0000-\u001f]/g, '')
    .replace(/[\s~`!@#$%^&*()\-_+=[\]{}|\\;:"'“”‘’<>,.?/]+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-+|-+$/g, '')
    .replace(/^(\d)/, '_$1')
    .toLowerCase()
    .normalize('NFC');
}

// Ref: https://vitepress.dev/reference/site-config
const vitePressConfig: UserConfig = {
  title: 'DaruDB',
  lastUpdated: true,
  outDir: '../docs-dist',
  cleanUrls: true,
  metaChunk: true,
  /**
   * The default locale is served from `/`, not from `/{lang}/`.
   *
   * This has to agree with two other things or every sidebar link 404s:
   * `vitepress-i18n` puts the root locale in `locales.root` (no path prefix),
   * and `vitepress-sidebar` is told to resolve its links against `/`. The
   * rewrite is what actually moves `docs/{defaultLocale}/**` there. Every other
   * locale keeps its folder as its prefix. Switching `defaultLocale` swings all
   * three together.
   */
  rewrites: {
    [`${defaultLocale}/:rest*`]: ':rest*'
  },
  head: [
    // The logo at the sizes each consumer asks for: a 32-pixel PNG for a
    // browser tab, the `.ico` that some clients fetch from the root without
    // reading any tag, and the larger square a phone puts on its home screen.
    ['link', { rel: 'icon', type: 'image/png', sizes: '32x32', href: '/logo-32.png' }],
    ['link', { rel: 'icon', type: 'image/png', sizes: '16x16', href: '/logo-16.png' }],
    ['link', { rel: 'shortcut icon', href: '/favicon.ico' }],
    ['link', { rel: 'apple-touch-icon', sizes: '180x180', href: '/apple-touch-icon.png' }],
    // The half of the metadata that is the same on every page. The other half —
    // the canonical URL, the title, the description, the locale alternates — is
    // per page and lives in `transformHead`. The image is the square logo, which
    // is what a `summary` card shows; it has to be an absolute URL.
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'DaruDB' }],
    ['meta', { property: 'og:image', content: `${siteUrl}/logo-256.png` }],
    ['meta', { property: 'og:image:width', content: '256' }],
    ['meta', { property: 'og:image:height', content: '256' }],
    ['meta', { property: 'og:image:alt', content: 'The DaruDB logo' }],
    ['meta', { name: 'twitter:card', content: 'summary' }],
    // Which programming language every page is read in, applied to `<html>`
    // before the first paint, and the sidebar entries that language does not
    // have. See `languages.ts`.
    ['script', {}, languageHeadScript(singleLanguagePages, localePrefixes)],
    ['style', {}, sidebarStyle()]
  ],
  /**
   * `::: lang rust` … `:::`, the part of a page that one language's readers see.
   *
   * Every language's blocks stay in the document and CSS shows the reader's,
   * which is what makes the switch instant and keeps the languages' versions of
   * a page in one file, where they cannot drift apart. A block several
   * languages share is written `::: lang rust node`. A name that is not a
   * language fails the build rather than hiding the block from everyone.
   */
  markdown: {
    anchor: { slugify: slugifyHeading },
    headers: { slugify: slugifyHeading },
    config(md: MarkdownRenderer) {
      md.use(container, 'lang', {
        validate: (params: string) => /^lang(\s+\S+)+$/.test(params.trim()),
        render(tokens: { nesting: number; info: string }[], index: number) {
          const token = tokens[index];

          if (token.nesting !== 1) {
            return '</div>\n';
          }

          const ids = token.info.trim().split(/\s+/).slice(1);
          const unknown = ids.filter((id) => !(LANGUAGE_IDS as string[]).includes(id));

          if (unknown.length) {
            throw new Error(`::: lang names unknown languages: ${unknown.join(', ')}`);
          }

          return `<div class="lang-only" data-code-lang="${ids.join(' ')}">\n`;
        }
      });
    }
  },
  sitemap: {
    hostname: packageJson.homepage
  },
  /**
   * `robots.txt` and `llms.txt`, written rather than committed.
   *
   * Both name URLs derived from `package.json`, and a copy of either sitting in
   * `public/` would be one more place to forget when the site moves.
   *
   * The crawler list is the split every vendor now documents: one that collects
   * pages to train a model takes the content and gives nothing back, and one
   * that fetches a page because somebody asked an assistant about it, or that
   * indexes the site so an assistant can find it, is how this site stays
   * reachable. The first kind is refused and the second is not. A token that is
   * not listed is covered by the `*` rule, which allows everything.
   */
  async buildEnd({ outDir }) {
    const refused = ['GPTBot', 'ClaudeBot', 'Google-Extended'];
    const allowed = ['ChatGPT-User', 'Claude-User', 'OAI-SearchBot', 'Claude-SearchBot'];
    const rules = [
      ...refused.map((bot) => `User-agent: ${bot}\nDisallow: /`),
      ...allowed.map((bot) => `User-agent: ${bot}\nAllow: /`),
      'User-agent: *\nAllow: /'
    ];

    await writeFile(
      resolve(outDir, 'robots.txt'),
      `${rules.join('\n\n')}\n\nSitemap: ${siteUrl}/sitemap.xml\n`
    );
    await writeFile(resolve(outDir, 'llms.txt'), llmsTxt());
  },
  /**
   * A description that is about this page rather than about the project.
   *
   * Runs in the dev server as well as in the build, which is what makes it the
   * right place for the description — `transformHead` would have to repeat the
   * fallback chain VitePress already applies to `pageData.description`.
   */
  transformPageData(pageData) {
    if (!pageData.description && pageData.filePath) {
      pageData.description = summaryOf(pageData.filePath) ?? '';
    }

    // The previous and next links, from the pages of the reader's language.
    // A page that names its own keeps them.
    const neighbours = pageData.filePath ? neighboursOf(pathOf(pageData.filePath)) : undefined;

    if (neighbours) {
      pageData.frontmatter.prev ??= neighbours.prev;
      pageData.frontmatter.next ??= neighbours.next;
    }
  },
  transformHead,
  themeConfig: {
    // Beside the site title rather than in place of it: the mark is a picture,
    // and the name is what a reader searches for. 64 pixels for a slot of 24,
    // so that it stays sharp on a high-density screen.
    logo: { src: '/logo-64.png', alt: '' },
    // Read by the language switch and the layout. See `languages.ts`.
    languagePages,
    // The marks the language switch and the package menu draw. See `icons.ts`.
    languageIcons: languageIcons(),
    // The package menu beside the GitHub link, which takes the place of a
    // social link for each registry. See `PackageMenu.vue`.
    registries,
    /**
     * `h2` and `h3`, nested. A guide page is a handful of `h2`s with the steps
     * or options as `h3`s under them, and the thing a reader came for is often
     * one of those. At the default depth it is never in the outline.
     */
    outline: { level: [2, 3] },
    editLink: {
      pattern: editLinkPattern
    },
    socialLinks: [{ icon: 'github', link: repoUrl }],
    footer: {
      message: 'Released under the MIT License',
      copyright: '© <a href="https://cdget.com">CDGet</a>'
    }
  }
};

/* ---------------------------------------------------------------------------
 * Sidebar post-processing
 *
 * `vitepress-sidebar` derives the menu from the folder tree, which cannot name
 * a group whose folder has no `index.md` or put a loose page under a heading.
 * So the generated tree is reshaped here instead, once, for every locale.
 * ------------------------------------------------------------------------- */

interface GeneratedSidebarItem {
  text?: string;
  link?: string;
  items?: GeneratedSidebarItem[];
  collapsed?: boolean;
}

/**
 * `useFolderLinkFromIndexFile` points a folder at its `index.md`, which
 * VitePress resolves to `/folder/index` — a URL that only works because the
 * SPA router is forgiving about it. The canonical one, and the only one a
 * static host serves directly, is `/folder/`.
 *
 * `collapsed` goes at the same time: VitePress draws the expand/collapse caret
 * for any item where `collapsed != null`, so the only way to have permanently
 * open groups with no toggle is for the key to be absent entirely.
 */
function cleanUpItems<T extends GeneratedSidebarItem>(items: T[]): T[] {
  return items.map((item) => {
    const cleaned = {
      ...item,
      ...(item.link ? { link: item.link.replace(/(^|\/)index\.md$/, '$1') } : {}),
      ...(item.items ? { items: cleanUpItems(item.items) } : {})
    };

    delete cleaned.collapsed;

    return cleaned;
  });
}

/** The first link anywhere in a subtree — how a group is identified below. */
function firstLink(item: GeneratedSidebarItem): string | undefined {
  return item.link ?? item.items?.map(firstLink).find(Boolean);
}

const startsWith = (prefix: string) => (item: GeneratedSidebarItem) =>
  firstLink(item)?.startsWith(prefix) ?? false;

/**
 * The groups in the order a reader needs them: the guide, the engine, the API
 * and the types, and last a heading of its own over the changelog, the
 * comparison with other databases, and the guides to moving data in from one.
 *
 * The API and Types sections hold one folder per language. The folders are
 * not shown as groups: their pages go straight under the section's heading,
 * and the reader's language hides the other folders' entries (`languages.css`),
 * so a reader on Node.js sees the Node.js API and nothing else. The changelog
 * has a page per language and is shown the same way, as one entry, without the
 * overview that sends a reader on to it.
 */
function arrangeSidebar<T extends GeneratedSidebarItem>(items: T[], lang: string): T[] {
  const labels = groupLabels[lang] ?? groupLabels[defaultLocale];
  // Links are relative to the locale's `base`, so `guide/introduction` in
  // every locale.
  const find = (section: string) => items.find(startsWith(section));

  const guide = find('guide/');
  const engine = find('engine/');
  const api = find('api/');
  const types = find('types/');
  const changelog = find('changelog/');
  const compare = find('compare');
  const migration = find('migration/');

  const titled: [T | undefined, string][] = [
    [guide, labels.guide],
    [engine, labels.engine],
    [api, labels.api],
    [types, labels.types],
    [migration, labels.migration]
  ];

  for (const [group, text] of titled) {
    if (group) {
      group.text = text;
    }
  }

  for (const [group, section] of [
    [api, 'api'],
    [types, 'types']
  ] as const) {
    if (group?.items) {
      group.items = LANGUAGE_IDS.flatMap(
        (id) => group.items?.find(startsWith(`${section}/${id}/`))?.items ?? []
      );
    }
  }

  const changelogs = LANGUAGE_IDS.flatMap((id) =>
    (changelog?.items ?? [])
      .filter((item) => item.link === `changelog/${id}`)
      .map((item) => ({ ...item, text: labels.changelog }) as T)
  );

  const loose = [...changelogs, compare, migration].filter(Boolean) as T[];
  const more = loose.length ? ({ text: labels.more, items: loose } as unknown as T) : undefined;
  const placed = [guide, engine, api, types, more].filter(Boolean) as T[];
  const moved = new Set<T | undefined>([...placed, changelog, compare, migration]);

  return [...placed, ...items.filter((item) => !moved.has(item))];
}

const config = withSidebar(withI18n(vitePressConfig, vitePressI18nConfig), vitePressSidebarConfig);

const sidebar = config.themeConfig?.sidebar as
  Record<string, { items?: GeneratedSidebarItem[] } | GeneratedSidebarItem[]> | undefined;

if (sidebar) {
  for (const [path, group] of Object.entries(sidebar)) {
    // `/` is the default locale and `/{lang}/` is every other one — the same
    // mapping `localeBase` makes, read back the other way.
    const lang = path === '/' ? defaultLocale : path.replaceAll('/', '');

    if (Array.isArray(group)) {
      sidebar[path] = arrangeSidebar(cleanUpItems(group), lang);
    } else if (group?.items) {
      group.items = arrangeSidebar(cleanUpItems(group.items), lang);
    }
  }
}

/* ---------------------------------------------------------------------------
 * Previous and next
 *
 * The default theme links each page to its neighbours in the sidebar. With the
 * API and Types sections holding every language's pages in one list, that
 * would send a reader of the last Rust page on to the first Node.js one, which
 * the sidebar hides from them. So each page's neighbours are chosen here from
 * the pages its own readers can see: a page for every language links to the
 * next page for every language, and a page for some languages also to pages
 * for those.
 * ------------------------------------------------------------------------- */

interface SidebarLink {
  text: string;
  link: string;
}

/** Every link in a sidebar, in the order a reader meets them, as absolute paths. */
function flatten(items: GeneratedSidebarItem[], base: string): SidebarLink[] {
  return items.flatMap((item) => [
    ...(item.link && item.text
      ? [{ text: item.text, link: item.link.startsWith('/') ? item.link : `${base}${item.link}` }]
      : []),
    ...flatten(item.items ?? [], base)
  ]);
}

const sidebarLinks: Record<string, SidebarLink[]> = Object.fromEntries(
  Object.entries(sidebar ?? {}).map(([path, group]) => [
    path,
    Array.isArray(group)
      ? flatten(group, path)
      : flatten(group?.items ?? [], (group as { base?: string })?.base ?? path)
  ])
);

/** The page's languages, from its locale-prefixed path. */
function readersOf(path: string): LanguageId[] {
  const route = path.replace(new RegExp(`^/(?:${localePrefixes.join('|')})(?=/|$)`), '') || '/';

  return languagesOf(route.replace(/\/+$/, '') || '/', languagePages);
}

function neighboursOf(
  path: string
): { prev: SidebarLink | false; next: SidebarLink | false } | undefined {
  const first = path.split('/')[1] ?? '';
  const links = sidebarLinks[localePrefixes.includes(first) ? `/${first}/` : '/'] ?? [];
  const normalized = (link: string) => link.replace(/\/+$/, '') || '/';
  const at = links.findIndex((item) => normalized(item.link) === normalized(path));

  if (at < 0) {
    return undefined;
  }

  const mine = readersOf(path);
  const visible = (item: SidebarLink) => {
    const theirs = readersOf(item.link);

    return mine.every((id) => theirs.includes(id));
  };

  const prev = links.slice(0, at).reverse().find(visible);
  const next = links.slice(at + 1).find(visible);

  return { prev: prev ?? false, next: next ?? false };
}

export default defineConfig(config);
