<script setup lang="ts">
import DefaultTheme from 'vitepress/theme';
import { onContentUpdated, useData, useRoute } from 'vitepress';
import { computed, nextTick, onMounted, watch } from 'vue';
import { isAvailable, languagesOf, routeOf, type LanguagePages } from '../languages';
import LanguageSwitch from './LanguageSwitch.vue';
import PackageMenu from './PackageMenu.vue';
import { language, setLanguage, syncLanguage } from './language';

/*
 * The default layout, with the language switch above the sidebar, the package
 * menu in the navbar, and the two things the default theme draws from data
 * rather than from the page, which a `::: lang` block cannot reach on its own.
 */

const { Layout } = DefaultTheme;
const { site, theme } = useData();
const route = useRoute();

const localePrefixes = computed(() =>
  Object.keys(site.value.locales).filter((key) => key !== 'root')
);

/**
 * A page written for other languages than the reader's switches the reader to
 * its own. The script in `<head>` does the same on a full load; this covers a
 * navigation inside the app.
 */
function followPage() {
  const current = routeOf(route.path, localePrefixes.value);
  const languages = languagesOf(current, theme.value.languagePages as LanguagePages);

  if (!languages.includes(language.value)) {
    const next = languages.find(isAvailable);

    if (next) {
      setLanguage(next);
    }
  }
}

/**
 * Hides the "On this page" entries whose heading sits in a block of another
 * language. The outline is built from the Markdown, and the blocks only exist
 * in the DOM, so this is the one place the two can be matched.
 */
function syncOutline() {
  const doc = document.querySelector('.vp-doc');

  if (!doc) {
    return;
  }

  for (const link of document.querySelectorAll<HTMLAnchorElement>(
    '.VPDocAsideOutline .outline-link'
  )) {
    const id = decodeURIComponent(link.getAttribute('href')?.slice(1) ?? '');
    const heading = id ? doc.querySelector(`[id="${CSS.escape(id)}"]`) : null;
    const block = heading?.closest<HTMLElement>('.lang-only');
    const hidden = Boolean(block) && !block!.dataset.codeLang!.split(' ').includes(language.value);

    (link.closest('li') ?? link).classList.toggle('lang-hidden', hidden);
  }
}

// The stored choice is read after mounting, not before, so that the first
// client render matches the pre-rendered page.
onMounted(() => {
  syncLanguage();
  followPage();
  nextTick(syncOutline);
});

watch(
  () => route.path,
  () => followPage()
);

// `onContentUpdated` is what the outline itself is built on, so it runs on the
// first render and after every navigation.
onContentUpdated(() => nextTick(syncOutline));
watch(language, () => nextTick(syncOutline));
</script>

<template>
  <Layout>
    <template #nav-bar-content-after>
      <PackageMenu />
    </template>
    <template #nav-screen-content-after>
      <PackageMenu variant="screen" />
    </template>
    <template #sidebar-nav-before>
      <LanguageSwitch />
    </template>
  </Layout>
</template>
