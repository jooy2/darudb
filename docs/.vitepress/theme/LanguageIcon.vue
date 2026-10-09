<script setup lang="ts">
import { useData } from 'vitepress';
import { computed } from 'vue';
import type { LanguageId } from '../languages';

/*
 * A language's mark, beside its name in the language switch. Decorative: the
 * name next to it is what a screen reader reads.
 *
 * The SVG body comes from the theme config, which `icons.ts` fills from the
 * icon set at build time, so it is the site's own data rather than anything a
 * page or a reader supplies.
 */

const props = defineProps<{ id: LanguageId }>();
const { theme } = useData();

const body = computed(
  () => (theme.value.languageIcons as Record<LanguageId, string> | undefined)?.[props.id] ?? ''
);
</script>

<template>
  <svg
    class="lang-icon"
    :data-icon="id"
    viewBox="0 0 24 24"
    aria-hidden="true"
    focusable="false"
    v-html="body"
  />
</template>
