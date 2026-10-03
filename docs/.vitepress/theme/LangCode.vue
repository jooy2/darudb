<script setup lang="ts">
import { CODE_LANGUAGES } from '../languages';

/*
 * A name that each language spells its own way, inline in a sentence that is
 * otherwise the same for all of them:
 *
 *   <LangCode rust="Database::open" node="Database.open" />
 *
 * Every spelling is rendered and CSS shows the reader's, as a `::: lang` block
 * does. A language left out shows nothing, so pass every language the page is
 * for.
 */

// One prop per language, written out: the compiler that reads `defineProps`
// cannot resolve a mapped type.
const props = defineProps<{ rust?: string; node?: string; dart?: string }>();

const spellings = CODE_LANGUAGES.filter((item) => props[item.id] !== undefined).map((item) => ({
  id: item.id,
  text: props[item.id] as string
}));
</script>

<template>
  <code
    v-for="spelling in spellings"
    :key="spelling.id"
    class="lang-only"
    :data-code-lang="spelling.id"
    >{{ spelling.text }}</code
  >
</template>
