/**
 * The programming languages DaruDB ships for, and how the site remembers which
 * one a reader uses.
 *
 * This is the site's second axis. The locale decides which human language a
 * page is written in; the programming language decides which package it
 * describes, so a reader on Node.js sees TypeScript examples and the Node.js
 * reference without leaving the page they are on.
 *
 * Shared by the config, which renders `::: lang` blocks and writes the script
 * in `<head>`, and by the theme, which draws the switch.
 */

export type LanguageId = 'rust' | 'node' | 'dart' | 'python';

export interface CodeLanguage {
  id: LanguageId;
  label: string;
  /**
   * Whether the package exists yet. A planned one is listed in the switch, so
   * that a reader sees it is coming, and cannot be chosen.
   */
  available: boolean;
}

export const CODE_LANGUAGES: CodeLanguage[] = [
  { id: 'rust', label: 'Rust', available: true },
  { id: 'node', label: 'Node.js', available: true },
  { id: 'dart', label: 'Dart', available: true },
  { id: 'python', label: 'Python', available: true }
];

export const LANGUAGE_IDS: LanguageId[] = CODE_LANGUAGES.map((language) => language.id);

export const AVAILABLE_LANGUAGE_IDS: LanguageId[] = CODE_LANGUAGES.filter(
  (language) => language.available
).map((language) => language.id);

/** The first in the list, which is the order the project names them in everywhere. */
export const DEFAULT_LANGUAGE: LanguageId = 'rust';

/** Where the choice is kept. It applies to the whole site, so not to the URL. */
export const LANGUAGE_STORAGE_KEY = 'darudb-language';

/**
 * The sections whose pages are each written for one language, in a folder
 * named after it: `/api/rust/database`, `/types/node/key`. Every other page is
 * shared, with `::: lang` blocks where the languages differ, unless its
 * frontmatter lists `languages`.
 */
export const PER_LANGUAGE_SECTIONS = ['api', 'types'];

/** What the theme config carries about pages that are not for every language. */
export interface LanguagePages {
  /** Shared-section pages written for some languages only, from their frontmatter. */
  only: Record<string, LanguageId[]>;
  /** Every page of the per-language sections, so that a counterpart can be looked up. */
  reference: string[];
  /** A page's counterpart in another language, where the two are named differently. */
  counterparts: Record<string, string[]>;
}

export function isAvailable(value: unknown): value is LanguageId {
  return typeof value === 'string' && (AVAILABLE_LANGUAGE_IDS as string[]).includes(value);
}

/**
 * A path as the site's routes are keyed: no locale prefix, no extension, no
 * trailing slash. `/ko/api/rust/database.html#open` → `/api/rust/database`.
 */
export function routeOf(path: string, localePrefixes: string[]): string {
  const prefix = new RegExp(`^/(?:${localePrefixes.join('|')})(?=/|$)`);
  const route = path
    .replace(/[?#].*$/, '')
    .replace(/(\/index)?\.html$/, '')
    .replace(/\/+$/, '')
    .replace(prefix, '');

  return route || '/';
}

/** The language a route's folder names, for a page of a per-language section. */
export function languageOfRoute(route: string): LanguageId | null {
  const match = new RegExp(`^/(?:${PER_LANGUAGE_SECTIONS.join('|')})/([a-z]+)(?:/|$)`).exec(route);

  return match && (LANGUAGE_IDS as string[]).includes(match[1]) ? (match[1] as LanguageId) : null;
}

/** The languages a page is written for: one, some, or every language. */
export function languagesOf(route: string, pages: LanguagePages | undefined): LanguageId[] {
  const folder = languageOfRoute(route);

  if (folder) {
    return [folder];
  }

  return pages?.only[route] ?? LANGUAGE_IDS;
}

/**
 * Where a reader of `route` goes after switching to `target`: the same page
 * when it is written for `target`, its counterpart when it has one, the same
 * slug in `target`'s folder, and the section's overview after that. `null`
 * keeps the reader where they are.
 */
export function counterpartOf(
  route: string,
  target: LanguageId,
  pages: LanguagePages | undefined
): string | null {
  if (languagesOf(route, pages).includes(target)) {
    return null;
  }

  const named = pages?.counterparts[route]?.find((other) => languageOfRoute(other) === target);

  if (named) {
    return named;
  }

  const folder = languageOfRoute(route);

  if (!folder) {
    return null;
  }

  const sameSlug = route.replace(`/${folder}/`, `/${target}/`);

  if (pages?.reference.includes(sameSlug)) {
    return sameSlug;
  }

  return `/${route.split('/')[1]}/`;
}

/**
 * The script in `<head>` that writes the reader's language onto `<html>`
 * before the first paint.
 *
 * Every language's content is in the document and CSS shows one of them, so
 * this attribute is what a page is read through. It has to be set by a
 * blocking script rather than by the app, or a reader who chose Node.js would
 * watch the Rust examples appear and vanish on every load.
 *
 * A page written for one language sets that language, and stores it: the
 * reader went to that page, and the sidebar and examples around it should
 * agree with it. `singleLanguagePages` lists those outside the per-language
 * sections, whose folder already says.
 */
export function languageHeadScript(
  singleLanguagePages: Record<string, LanguageId>,
  localePrefixes: string[]
): string {
  const ids = JSON.stringify(AVAILABLE_LANGUAGE_IDS);
  const key = JSON.stringify(LANGUAGE_STORAGE_KEY);
  const fallback = JSON.stringify(DEFAULT_LANGUAGE);
  const pages = JSON.stringify(singleLanguagePages);
  const locale = JSON.stringify(`^/(?:${localePrefixes.join('|')})(?=/|$)`);
  const section = JSON.stringify(`^/(?:${PER_LANGUAGE_SECTIONS.join('|')})/([a-z]+)(?:/|$)`);

  return `(function(){var ids=${ids},key=${key},stored,route=location.pathname.replace(/(\\/index)?\\.html$/,'').replace(/\\/+$/,'').replace(new RegExp(${locale}),'')||'/',match=new RegExp(${section}).exec(route),forced=match?match[1]:${pages}[route];try{stored=localStorage.getItem(key)}catch(e){}var lang=ids.indexOf(forced)>=0?forced:ids.indexOf(stored)>=0?stored:${fallback};if(lang===forced&&lang!==stored){try{localStorage.setItem(key,lang)}catch(e){}}document.documentElement.dataset.codeLang=lang})()`;
}
