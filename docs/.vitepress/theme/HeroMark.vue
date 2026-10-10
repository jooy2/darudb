<script setup lang="ts">
import { useData, withBase } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import LanguageIcon from './LanguageIcon.vue';

/*
 * The logo in the home page's hero, with the four languages DaruDB ships for
 * circling it.
 *
 * The ring says what the hero's heading says, that one engine sits under every
 * package, without asking the reader to read it. It is decoration, so it is
 * hidden from screen readers; the heading beside it carries the same words.
 *
 * The image itself comes from the page's `hero.image`, so each locale keeps
 * its own alt text there, and the slot this fills falls back to nothing else.
 * Every animation stops for a reader who asked for less motion.
 */

const { frontmatter } = useData();

const image = computed(() => {
  const hero = frontmatter.value.hero as { image?: { src?: string; alt?: string } } | undefined;

  return { src: withBase(hero?.image?.src ?? '/logo.webp'), alt: hero?.image?.alt ?? '' };
});

/** Each language's place on the ring, as an angle from the top, clockwise. */
const badges = CODE_LANGUAGES.map((language, index) => ({
  ...language,
  angle: (360 / CODE_LANGUAGES.length) * index + 45
}));
</script>

<template>
  <div class="hero-mark">
    <div class="hero-mark-orbit" aria-hidden="true">
      <svg class="hero-mark-rings" viewBox="0 0 100 100">
        <circle class="hero-mark-ring outer" cx="50" cy="50" r="47" />
        <circle class="hero-mark-ring inner" cx="50" cy="50" r="38" />
      </svg>
      <span
        v-for="badge in badges"
        :key="badge.id"
        class="hero-mark-badge"
        :style="{ '--angle': `${badge.angle}deg` }"
      >
        <span class="hero-mark-badge-face" :title="badge.label">
          <LanguageIcon :id="badge.id" />
        </span>
      </span>
    </div>
    <img class="hero-mark-logo" :src="image.src" :alt="image.alt" width="256" height="256" />
  </div>
</template>
