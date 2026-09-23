import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { withSidebar } from 'vitepress-sidebar';
import packageJson from '../../packages/node/package.json' with { type: 'json' };
import { defineConfig, HeadConfig, SiteData, TransformContext, UserConfig } from 'vitepress';
import { withI18n } from 'vitepress-i18n';
import type { VitePressI18nOptions } from 'vitepress-i18n/types';
import type { VitePressSidebarOptions } from 'vitepress-sidebar/types';

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

/**
 * What the site is about, in the default locale. The npm package has a
 * description of its own, but that one is about the Node.js binding, and this
 * site is about every package.
 */
const SITE_DESCRIPTION =
  "An embedded database that keeps an application's data in one local file, for Rust, Node.js and Dart. One engine written in Rust, with encryption, crash safety and several processes on one file as goals from the start.";

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
 * The sidebar groups the folder tree cannot name.
 *
 * `guide/` has no `index.md` and the changelog is a loose page, so neither can
 * take its heading from a page. Left to the generator, `guide/` would be
 * capitalised to "Guide" over Korean pages and the changelog would sit at the
 * root with no heading over it at all.
 */
const groupLabels: Record<string, { guide: string; more: string }> = {
  en: { guide: 'Guide', more: 'Discover more' },
  ko: { guide: '가이드', more: '더 알아보기' }
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
const navFor = (lang: string, labels: [string, string]) => [
  { text: labels[0], link: `${localeBase(lang)}guide/introduction` },
  { text: labels[1], link: `${localeBase(lang)}changelog` }
];

const vitePressI18nConfig: VitePressI18nOptions = {
  locales: supportLocales,
  rootLocale: defaultLocale,
  searchProvider: 'local',
  description: {
    en: SITE_DESCRIPTION,
    ko: '애플리케이션의 데이터를 로컬 파일 하나에 담는 임베디드 데이터베이스입니다. Rust로 작성한 엔진 하나를 Rust와 Node.js, Dart에서 함께 쓰며, 암호화와 크래시 안전성, 여러 프로세스의 동시 접근을 처음부터 목표로 설계합니다.'
  },
  themeConfig: {
    en: { nav: navFor('en', ['Guide', 'Changelog']) },
    ko: { nav: navFor('ko', ['가이드', '변경 기록']) }
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

/** Inline Markdown and HTML dropped: a `<meta>` carries text and nothing else. */
function plainText(source: string): string {
  return source
    .replace(/<[^>]+>/g, ' ')
    .replace(/!\[[^\]]*]\([^)]*\)/g, ' ')
    .replace(/\[([^\]]*)]\([^)]*\)/g, '$1')
    .replace(/[`*_]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Cut at a word boundary, to about what a result page will show whole. */
function clamp(text: string, limit = 160): string {
  if (text.length <= limit) {
    return text;
  }

  const cut = text.slice(0, limit);
  const boundary = cut.lastIndexOf(' ');

  return `${(boundary > 0 ? cut.slice(0, boundary) : cut).trimEnd()}…`;
}

/**
 * A page's own one-line summary: the first block that is prose rather than the
 * title, a fenced example, a table or a container.
 */
function summaryOf(filePath: string): string | undefined {
  const file = resolve(srcDir, filePath);

  if (!existsSync(file)) {
    return undefined;
  }

  const source = readFileSync(file, 'utf8');

  for (const block of source.replace(/^---\r?\n[\s\S]*?\r?\n---/, '').split(/\n\s*\n/)) {
    const trimmed = block.trim();

    if (!trimmed || /^[#<`:|>-]/.test(trimmed)) {
      continue;
    }

    const text = plainText(trimmed);

    if (text) {
      return clamp(text);
    }
  }

  return undefined;
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
    'package `darudb`, and later to Dart. Every language reads and writes the same file the same',
    'way, because every rule about the file lives in the engine. The project is in early',
    'development and nothing is published yet.',
    ''
  ];

  for (const [name, folder] of [['Guide', `${defaultLocale}/guide`]]) {
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
  lines.push(
    '## Optional',
    '',
    `- [Changelog](${siteUrl}${pathOf(`${defaultLocale}/changelog`)}): every package's changes, newest first`,
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
    sameAs: [repoUrl, npmUrl, cratesUrl]
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
    // The half of the metadata that is the same on every page. The other half —
    // the canonical URL, the title, the description, the locale alternates — is
    // per page and lives in `transformHead`. There is no `og:image` yet: the
    // project has no mark to put in one.
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'DaruDB' }],
    ['meta', { name: 'twitter:card', content: 'summary' }]
  ],
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
  },
  transformHead,
  themeConfig: {
    /**
     * `h2` and `h3`, nested. A guide page is a handful of `h2`s with the steps
     * or options as `h3`s under them, and the thing a reader came for is often
     * one of those. At the default depth it is never in the outline.
     */
    outline: { level: [2, 3] },
    editLink: {
      pattern: editLinkPattern
    },
    socialLinks: [
      { icon: 'npm', link: npmUrl, ariaLabel: `${packageJson.name} on npm` },
      { icon: 'github', link: repoUrl }
    ],
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
 * The guide first, then anything loose under a heading of its own.
 *
 * The changelog is a loose page with nothing above it, so it is given a group:
 * the place anything that is neither a guide nor a reference ends up.
 */
function arrangeSidebar<T extends GeneratedSidebarItem>(items: T[], lang: string): T[] {
  const labels = groupLabels[lang] ?? groupLabels[defaultLocale];

  const guide = items.find(startsWith('guide/'));
  const changelog = items.find(startsWith('changelog'));

  if (guide) {
    guide.text = labels.guide;
  }

  const loose = [changelog].filter(Boolean) as T[];
  const more = loose.length ? ({ text: labels.more, items: loose } as unknown as T) : undefined;
  const moved = new Set([guide, changelog].filter(Boolean));

  return [...([guide, more].filter(Boolean) as T[]), ...items.filter((item) => !moved.has(item))];
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

export default defineConfig(config);
