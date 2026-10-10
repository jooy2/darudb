<script setup lang="ts">
import { useData, withBase } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES, type LanguageId } from '../languages';
import LanguageIcon from './LanguageIcon.vue';
import { setLanguage } from './language';
import { localeOf, t } from './i18n';

/*
 * One card for each language, with what it needs and the command that adds
 * the package, which open the getting-started page in that language:
 *
 *   <StartCards :cards="[{ id: 'rust', note: '...', install: 'cargo add darudb', link: '/guide/getting-started' }]" />
 *
 * Taking a card is the choice the language switch makes, so it is stored the
 * same way, and the page it opens shows that language's commands and code.
 */

const props = defineProps<{
  cards: { id: LanguageId; note: string; install: string; link: string }[];
}>();

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));

const cards = computed(() =>
  props.cards.flatMap((card) => {
    const language = CODE_LANGUAGES.find((item) => item.id === card.id && item.available);

    return language ? [{ ...card, label: language.label }] : [];
  })
);
</script>

<template>
  <div class="start-cards">
    <a
      v-for="card in cards"
      :key="card.id"
      class="start-card"
      :href="withBase(card.link)"
      :data-code-lang="card.id"
      @click="setLanguage(card.id)"
    >
      <span class="start-card-head">
        <LanguageIcon :id="card.id" />
        <span class="start-card-name">{{ card.label }}</span>
      </span>
      <span class="start-card-note">{{ card.note }}</span>
      <code class="start-card-install">{{ card.install }}</code>
      <span class="start-card-action">
        {{ t(locale, 'startAction') }}
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M5 12h13m-5-6 6 6-6 6"
          />
        </svg>
      </span>
    </a>
  </div>
</template>
