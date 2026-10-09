<script setup lang="ts">
import { useData, useRoute } from 'vitepress';
import { computed, nextTick, ref, watch } from 'vue';
import type { Registry } from '../languages';
import LanguageIcon from './LanguageIcon.vue';
import { localeOf, t } from './i18n';

/*
 * The registries DaruDB is published on, one icon in the navbar that opens a
 * list of them, beside the GitHub link.
 *
 * One icon rather than one per registry, since four registries and six
 * packages would crowd out the links the navbar is for. It is a disclosure, a
 * button that shows a group of links, rather than an ARIA menu: its entries
 * are links to other sites, which a reader moves through with Tab as they
 * would anywhere else.
 *
 * The navbar shows it from 768 pixels up. It opens on a click or a key, and
 * for a mouse also while the pointer is over it, as the theme's own flyouts
 * do; that part is CSS alone (`packages.css`), since a hover that set the
 * state would leave the click that follows it to close the list again. Below
 * 768 pixels the navbar is only the search and the hamburger, so the same list
 * is drawn open inside the screen the hamburger opens, as `variant="screen"`.
 */

const props = withDefaults(defineProps<{ variant?: 'bar' | 'screen' }>(), { variant: 'bar' });

const { lang, theme } = useData();
const route = useRoute();
const locale = computed(() => localeOf(lang.value));
const registries = computed(() => (theme.value.registries ?? []) as Registry[]);

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);

/** One id per instance, since the navbar and the screen may both be in the page. */
const panelId = `package-menu-${props.variant}`;

function close(refocus = false) {
  if (!open.value) {
    return;
  }

  open.value = false;

  if (refocus) {
    button.value?.focus();
  }
}

/** The arrow keys open the list and move into it, as they would a select. */
async function openAndFocus() {
  open.value = true;
  await nextTick();
  root.value?.querySelector<HTMLElement>('.package-menu-link')?.focus();
}

function onFocusOut(event: FocusEvent) {
  if (!root.value?.contains(event.relatedTarget as Node | null)) {
    close();
  }
}

// A link followed inside the site, or the browser's back button, leaves the
// list behind.
watch(
  () => route.path,
  () => close()
);
</script>

<template>
  <div
    v-if="variant === 'bar'"
    ref="root"
    class="package-menu"
    @focusout="onFocusOut"
    @keydown.esc="close(true)"
  >
    <button
      ref="button"
      type="button"
      class="package-menu-button"
      :aria-expanded="open"
      :aria-controls="panelId"
      :aria-label="t(locale, 'packages')"
      :title="t(locale, 'packages')"
      @click="open = !open"
      @keydown.down.prevent="openAndFocus()"
    >
      <!-- A box, for a package; drawn here rather than taken from an icon set. -->
      <svg class="package-menu-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <path
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M12 2.8 20.2 7.3v9.4L12 21.2 3.8 16.7V7.3L12 2.8ZM3.8 7.3 12 11.8l8.2-4.5M12 11.8v9.4M7.9 5l8.2 4.6"
        />
      </svg>
    </button>

    <div :id="panelId" class="package-menu-panel">
      <div class="package-menu-list">
        <p class="package-menu-heading">{{ t(locale, 'packagesHeading') }}</p>
        <div v-for="registry in registries" :key="registry.name" class="package-menu-group">
          <p class="package-menu-title">
            <LanguageIcon :id="registry.language" />
            <span>{{ registry.name }}</span>
          </p>
          <a
            v-for="item in registry.packages"
            :key="item.link"
            class="package-menu-link"
            :href="item.link"
            target="_blank"
            rel="noopener"
            >{{ item.name }}</a
          >
        </div>
      </div>
    </div>
  </div>

  <section v-else class="package-screen" :aria-labelledby="panelId">
    <p :id="panelId" class="package-screen-heading">{{ t(locale, 'packagesHeading') }}</p>
    <div v-for="registry in registries" :key="registry.name" class="package-screen-group">
      <p class="package-screen-title">
        <LanguageIcon :id="registry.language" />
        <span>{{ registry.name }}</span>
      </p>
      <ul>
        <li v-for="item in registry.packages" :key="item.link">
          <a :href="item.link" target="_blank" rel="noopener">{{ item.name }}</a>
        </li>
      </ul>
    </div>
  </section>
</template>
