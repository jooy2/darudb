<script setup lang="ts">
import { useData } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import { localeOf, t, type StringKey } from './i18n';
import { data as pages } from './reference.data';

/*
 * The pages of a per-language section, one list for each language, of which
 * CSS shows the reader's:
 *
 *   <PageList section="api" />
 *   <PageList section="types" grouped />
 *
 * Each entry is the page's title and its opening sentence. `grouped` draws a
 * table instead, sorted into the groups the pages' `group` frontmatter names,
 * in the order `GROUPS` gives them; a page without one goes last.
 */

const props = defineProps<{ section: string; grouped?: boolean }>();

/** The groups a page can name, in the order they are shown, with their labels. */
const GROUPS: [string, StringKey][] = [
  ['database', 'groupDatabase'],
  ['objects', 'groupObjects'],
  ['queries', 'groupQueries'],
  ['tools', 'groupTools'],
  ['errors', 'groupErrors']
];

/** A title that is a symbol's name is set as code; one such as "Field types" is not. */
const isIdentifier = (title: string) => /^[A-Za-z_$][\w$]*$/.test(title);
const { lang } = useData();
const locale = computed(() => localeOf(lang.value));

const groups = computed(() =>
  CODE_LANGUAGES.map((language) => ({
    id: language.id,
    pages: pages
      .filter(
        (page) =>
          page.locale === locale.value &&
          page.section === props.section &&
          page.language === language.id
      )
      .sort((a, b) => a.order - b.order || a.title.localeCompare(b.title))
  })).filter((group) => group.pages.length > 0)
);

/** The pages of one language, in their groups. */
function inGroups(list: typeof pages) {
  const known = GROUPS.map(([id, label]) => ({
    id,
    label: t(locale.value, label),
    pages: list.filter((page) => page.group === id)
  }));
  const rest = list.filter((page) => !GROUPS.some(([id]) => id === page.group));

  return [...known, { id: '', label: '', pages: rest }].filter((group) => group.pages.length > 0);
}
</script>

<template>
  <div
    v-for="group in groups"
    :key="group.id"
    class="lang-only page-list"
    :data-code-lang="group.id"
  >
    <table v-if="grouped" class="page-table">
      <thead>
        <tr>
          <th scope="col">{{ t(locale, 'pageListName') }}</th>
          <th scope="col">{{ t(locale, 'pageListSummary') }}</th>
        </tr>
      </thead>
      <tbody v-for="part in inGroups(group.pages)" :key="part.id">
        <tr v-if="part.label" class="page-table-group">
          <th colspan="2" scope="colgroup">{{ part.label }}</th>
        </tr>
        <tr v-for="page in part.pages" :key="page.url">
          <td>
            <a :href="page.url"
              ><code v-if="isIdentifier(page.title)">{{ page.title }}</code
              ><template v-else>{{ page.title }}</template></a
            >
          </td>
          <td>{{ page.summary }}</td>
        </tr>
      </tbody>
    </table>
    <ul v-else>
      <li v-for="page in group.pages" :key="page.url">
        <a :href="page.url"
          ><code v-if="isIdentifier(page.title)">{{ page.title }}</code
          ><template v-else>{{ page.title }}</template></a
        >
        <span class="page-list-summary">{{ page.summary }}</span>
      </li>
    </ul>
  </div>
</template>
