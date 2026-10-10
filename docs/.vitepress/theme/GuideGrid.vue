<script setup lang="ts">
import { withBase } from 'vitepress';

/*
 * A grid of links to guide pages, each with a line on what the page covers:
 *
 *   <GuideGrid :items="[{ title: 'Queries', desc: '...', link: '/guide/queries' }]" />
 *
 * The words and the links come from the page, which is written once per
 * locale, so each locale's page links to its own guide.
 *
 * Each card is one link rather than a card with a link inside it, so that the
 * whole card can be taken and a screen reader reads it as one thing.
 */

defineProps<{ items: { title: string; desc: string; link: string }[] }>();
</script>

<template>
  <ul class="guide-grid">
    <li v-for="item in items" :key="item.link + item.title">
      <a class="guide-card" :href="withBase(item.link)">
        <span class="guide-card-title">{{ item.title }}</span>
        <span class="guide-card-desc">{{ item.desc }}</span>
      </a>
    </li>
  </ul>
</template>
