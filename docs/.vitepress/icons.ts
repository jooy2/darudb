/**
 * The marks of the languages DaruDB ships for, which the language switch
 * draws beside each language's name.
 *
 * They come from Simple Icons (CC0-1.0), through the same iconify package
 * VitePress draws its social links from, and they are read here, at build
 * time, rather than imported by a component: the set holds thousands of icons
 * and the site needs four, and an import would ship all of them to every
 * reader. What reaches the theme is each icon's SVG body, in its config.
 */

import icons from '@iconify-json/simple-icons/icons.json' with { type: 'json' };
import type { LanguageId } from './languages';

/** Each language's icon in the set, by the set's own name for it. */
const ICON_NAMES: Record<LanguageId, string> = {
  rust: 'rust',
  node: 'nodedotjs',
  dart: 'dart',
  python: 'python'
};

/**
 * The inside of each language's 24 by 24 `<svg>`, filled with `currentColor`.
 *
 * A name the set no longer has fails the build, rather than leaving a blank
 * where the mark was.
 */
export function languageIcons(): Record<LanguageId, string> {
  const set = icons as { icons: Record<string, { body: string }>; width?: number; height?: number };

  if ((set.width ?? 16) !== 24 || (set.height ?? 16) !== 24) {
    throw new Error('The icon set is no longer drawn on a 24 by 24 grid.');
  }

  return Object.fromEntries(
    Object.entries(ICON_NAMES).map(([id, name]) => {
      const icon = set.icons[name];

      if (!icon) {
        throw new Error(`The icon set has no icon named ${name}.`);
      }

      return [id, icon.body];
    })
  ) as Record<LanguageId, string>;
}
