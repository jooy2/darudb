<script setup lang="ts">
import { useData } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import LanguageIcon from './LanguageIcon.vue';
import { language, setLanguage } from './language';
import { localeOf, t } from './i18n';

/*
 * The language switch for a page without a sidebar, which is the home page.
 *
 * It writes the same choice the sidebar's switch does, so a language taken here
 * is the one the guide and the API open in. Which tab is current is drawn by
 * CSS from `<html data-code-lang>`, which the script in `<head>` sets before the
 * first paint, rather than from the ref, which is only right after hydration;
 * `aria-pressed` reads the ref, which is corrected on mount.
 */

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));
const available = CODE_LANGUAGES.filter((item) => item.available);
</script>

<template>
  <div class="lang-tabs" role="group" :aria-label="t(locale, 'switchList')">
    <span class="lang-tabs-label">{{ t(locale, 'examplesIn') }}</span>
    <span class="lang-tabs-list">
      <button
        v-for="item in available"
        :key="item.id"
        type="button"
        class="lang-tab"
        :data-code-lang="item.id"
        :aria-pressed="item.id === language"
        @click="setLanguage(item.id)"
      >
        <LanguageIcon :id="item.id" />
        <span>{{ item.label }}</span>
      </button>
    </span>
  </div>
</template>
