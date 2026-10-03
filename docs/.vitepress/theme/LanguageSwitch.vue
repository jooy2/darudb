<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useData, useRoute, useRouter } from 'vitepress';
import {
  CODE_LANGUAGES,
  counterpartOf,
  isAvailable,
  routeOf,
  type LanguagePages,
  type LanguageId
} from '../languages';
import { language, setLanguage } from './language';
import { localeOf, t } from './i18n';

/*
 * The programming language switch, at the top of the sidebar.
 *
 * It sits in the sidebar rather than in the navbar because it is not
 * navigation: it changes what the page a reader is on says, and which
 * reference pages the sidebar below it lists.
 *
 * The button's face is not rendered from the ref. Every label is inside it and
 * CSS shows the chosen one, the way a `::: lang` block works, because the
 * attribute on `<html>` is set before the first paint and the ref only after
 * hydration. A face rendered from the ref would name the default language to a
 * reader who chose another for as long as the bundle takes to load. The menu
 * is rendered only while open, so it can read the ref directly.
 */

const { lang, site, theme } = useData();
const route = useRoute();
const router = useRouter();
const locale = computed(() => localeOf(lang.value));

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);

/** The locale folders, which a route is keyed without. */
const localePrefixes = computed(() =>
  Object.keys(site.value.locales).filter((key) => key !== 'root')
);

function options(): HTMLElement[] {
  return root.value ? [...root.value.querySelectorAll<HTMLElement>('[role="option"]')] : [];
}

function focusOption(index: number) {
  const all = options();

  if (all.length) {
    all[(index + all.length) % all.length].focus();
  }
}

/** Opens on the current choice, which is where a reader expects to land. */
async function openMenu() {
  open.value = true;
  await nextTick();
  focusOption(CODE_LANGUAGES.findIndex((item) => item.id === language.value));
}

function closeMenu(refocus = false) {
  if (!open.value) {
    return;
  }

  open.value = false;

  if (refocus) {
    button.value?.focus();
  }
}

/**
 * Switches, and moves a reader who is on a page written for the old language
 * to that page's counterpart, since a page that contradicts the switch is the
 * one thing the switch must never leave on screen.
 */
function choose(id: LanguageId) {
  if (!isAvailable(id)) {
    return;
  }

  closeMenu(true);

  if (id === language.value) {
    return;
  }

  setLanguage(id);

  const current = routeOf(route.path, localePrefixes.value);
  const target = counterpartOf(current, id, theme.value.languagePages as LanguagePages);

  if (target) {
    const prefix = localePrefixes.value.find(
      (key) => route.path === `/${key}` || route.path.startsWith(`/${key}/`)
    );

    router.go(`${prefix ? `/${prefix}` : ''}${target}`);
  }
}

function onMenuKey(event: KeyboardEvent) {
  const all = options();
  const at = all.indexOf(document.activeElement as HTMLElement);

  switch (event.key) {
    // An option is not activated by these keys on its own, whatever element
    // it is, so the list answers for it.
    case 'Enter':
    case ' ':
      event.preventDefault();

      if (at >= 0) {
        choose(CODE_LANGUAGES[at].id);
      }

      break;
    case 'ArrowDown':
      event.preventDefault();
      focusOption(at + 1);
      break;
    case 'ArrowUp':
      event.preventDefault();
      focusOption(at - 1);
      break;
    case 'Home':
      event.preventDefault();
      focusOption(0);
      break;
    case 'End':
      event.preventDefault();
      focusOption(all.length - 1);
      break;
    case 'Escape':
      event.preventDefault();
      closeMenu(true);
      break;
    case 'Tab':
      // Not prevented: focus is meant to leave, and the menu should not stay
      // open behind it.
      closeMenu();
      break;
  }
}

function onPointerDown(event: PointerEvent) {
  if (root.value && !root.value.contains(event.target as Node)) {
    closeMenu();
  }
}

onMounted(() => document.addEventListener('pointerdown', onPointerDown));
onBeforeUnmount(() => document.removeEventListener('pointerdown', onPointerDown));

// The sidebar outlives a navigation, and without this the menu would too.
watch(
  () => route.path,
  () => closeMenu()
);
</script>

<template>
  <div ref="root" class="lang-switch">
    <p id="lang-switch-label" class="lang-switch-title">{{ t(locale, 'switchLabel') }}</p>

    <button
      ref="button"
      type="button"
      class="lang-switch-button"
      aria-haspopup="listbox"
      :aria-expanded="open"
      aria-labelledby="lang-switch-label lang-switch-current"
      @click="open ? closeMenu() : openMenu()"
      @keydown.down.prevent="openMenu()"
      @keydown.up.prevent="openMenu()"
    >
      <span id="lang-switch-current" class="lang-switch-current">
        <span
          v-for="item in CODE_LANGUAGES.filter((entry) => entry.available)"
          :key="item.id"
          class="lang-only"
          :data-code-lang="item.id"
          >{{ item.label }}</span
        >
      </span>
      <svg class="lang-switch-chevron" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          d="m6 9 6 6 6-6"
        />
      </svg>
    </button>

    <div v-if="open" class="lang-switch-menu" @keydown="onMenuKey">
      <!-- The hint sits beside the list rather than in it: a listbox whose
           children are not all options leaves a screen reader guessing. -->
      <div class="lang-switch-list" role="listbox" :aria-label="t(locale, 'switchList')">
        <button
          v-for="item in CODE_LANGUAGES"
          :key="item.id"
          type="button"
          role="option"
          class="lang-switch-option"
          :aria-selected="item.id === language"
          :aria-disabled="!item.available"
          @click="choose(item.id)"
        >
          <span class="lang-switch-option-label">{{ item.label }}</span>
          <span v-if="!item.available" class="lang-switch-planned">{{ t(locale, 'planned') }}</span>
          <svg class="lang-switch-check" viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="none"
              stroke="currentColor"
              stroke-width="2.4"
              stroke-linecap="round"
              stroke-linejoin="round"
              d="m5 12.5 4.5 4.5L19 7"
            />
          </svg>
        </button>
      </div>

      <p class="lang-switch-hint">{{ t(locale, 'switchHint') }}</p>
    </div>
  </div>
</template>
