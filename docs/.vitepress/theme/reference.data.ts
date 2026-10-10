/**
 * Every page of the API and Types sections, for the lists on their overview
 * pages.
 *
 * Built from the pages that are there rather than kept by hand, so that a page
 * added to a section is in its overview the moment it exists. A data loader
 * rather than theme config, because only the two overview pages need it, and
 * the theme config is shipped with every page.
 */

import { createContentLoader } from 'vitepress';
import { LANGUAGE_IDS, type LanguageId } from '../languages';
import { summaryFromSource } from '../pages';

export interface ReferencePage {
  /** The page's route, with the locale prefix of a locale other than the default. */
  url: string;
  locale: string;
  section: string;
  language: LanguageId;
  title: string;
  summary: string;
  order: number;
  /** The `group` its frontmatter names, for an overview that sorts pages into groups. */
  group: string;
}

declare const data: ReferencePage[];
export { data };

/** The default locale, which is served from `/` rather than from its folder. */
const DEFAULT_LOCALE = 'en';

export default createContentLoader(['*/api/*/*.md', '*/types/*/*.md'], {
  includeSrc: true,
  transform(pages): ReferencePage[] {
    return pages.flatMap((page) => {
      // The URL may or may not have the rewrite that moves the default locale
      // to `/` applied, so both shapes are read the same way.
      const parts = page.url.split('/').filter(Boolean);
      const locale = parts.length === 4 ? parts.shift()! : DEFAULT_LOCALE;
      const [section, language, slug] = parts;

      if (!(LANGUAGE_IDS as string[]).includes(language)) {
        return [];
      }

      return [
        {
          url: `${locale === DEFAULT_LOCALE ? '' : `/${locale}`}/${section}/${language}/${slug}`,
          locale,
          section,
          language: language as LanguageId,
          title: String(page.frontmatter.title ?? slug),
          summary: summaryFromSource(page.src ?? '', 200) ?? '',
          order: Number(page.frontmatter.order ?? 99),
          group: String(page.frontmatter.group ?? '')
        }
      ];
    });
  }
});
