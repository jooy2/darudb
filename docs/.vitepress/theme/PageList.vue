<script setup lang="ts">
import { useData } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import { localeOf } from './i18n';
import { data as pages } from './reference.data';

/*
 * The pages of a per-language section, one list for each language, of which
 * CSS shows the reader's:
 *
 *   <PageList section="api" />
 *
 * Each entry is the page's title and its opening sentence.
 */

const props = defineProps<{ section: string }>();

/** A title that is a symbol's name is set as code; one such as "Field types" is not. */
const isIdentifier = (title: string) => /^[A-Za-z_$][\w$]*$/.test(title);
const { lang } = useData();

const groups = computed(() => {
  const locale = localeOf(lang.value);

  return CODE_LANGUAGES.map((language) => ({
    id: language.id,
    pages: pages
      .filter(
        (page) =>
          page.locale === locale && page.section === props.section && page.language === language.id
      )
      .sort((a, b) => a.order - b.order || a.title.localeCompare(b.title))
  })).filter((group) => group.pages.length > 0);
});
</script>

<template>
  <div
    v-for="group in groups"
    :key="group.id"
    class="lang-only page-list"
    :data-code-lang="group.id"
  >
    <ul>
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
