/**
 * The reader's programming language, as one value the whole site shares.
 *
 * `localStorage` keeps it between visits. While a page is open it lives in two
 * places, and each has a job:
 *
 * - `<html data-code-lang>` is what a page is shown through. Every language's
 *   content is in the document and CSS hides the others, so a switch costs one
 *   attribute write and no render.
 * - The ref below is for what Vue draws: the switch itself and the outline.
 *
 * `setLanguage` writes all three together.
 */

import { readonly, ref } from 'vue';
import { DEFAULT_LANGUAGE, isAvailable, LANGUAGE_STORAGE_KEY, type LanguageId } from '../languages';

const current = ref<LanguageId>(DEFAULT_LANGUAGE);

/**
 * The current choice. It stays at the default until `syncLanguage` runs on
 * mount, which keeps the first client render identical to the pre-rendered
 * page, as hydration requires.
 */
export const language = readonly(current);

export function setLanguage(next: LanguageId): void {
  if (!isAvailable(next)) {
    return;
  }

  current.value = next;
  document.documentElement.dataset.codeLang = next;

  try {
    localStorage.setItem(LANGUAGE_STORAGE_KEY, next);
  } catch {
    // Private browsing, or storage that is full. The choice still holds for
    // this page; it will not survive a reload.
  }
}

/**
 * The reader's language as the document has it, which is right before the app
 * has mounted as well as after.
 *
 * The attribute is read first, because the script in `<head>` has already
 * decided, for a page written for one language, which language it is. The dev
 * server can add the `head` config after the app has started, so storage is
 * the answer when the attribute is not there yet.
 */
export function chosenLanguage(): LanguageId {
  const applied = document.documentElement.dataset.codeLang;
  let stored: string | null = null;

  try {
    stored = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  } catch {
    // Private browsing: the attribute or the default will do.
  }

  return isAvailable(applied) ? applied : isAvailable(stored) ? stored : DEFAULT_LANGUAGE;
}

/** Reads the language the script in `<head>` applied into the ref. */
export function syncLanguage(): void {
  const next = chosenLanguage();

  current.value = next;
  document.documentElement.dataset.codeLang = next;
}
