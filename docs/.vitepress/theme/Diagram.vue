<script setup lang="ts">
import { computed } from 'vue';

/*
 * A diagram of the guide, drawn inline from an SVG in `diagrams/`:
 *
 *   <Diagram name="commits" alt="A sync commit waits for the disk ..." />
 *
 * The SVGs are drawn in English only, one for both locales, and say what they
 * mean through classes rather than colours, which `diagram.css` gives the
 * theme's own colours. That is why they are inlined rather than linked as
 * images: an `<img>` cannot see the page's stylesheet, and would stay light
 * when the reader switches the site to dark.
 *
 * `alt` is the page's own description of the picture, in the page's locale,
 * which is what a screen reader reads in its place.
 */

const sources = import.meta.glob<string>('./diagrams/*.svg', {
  query: '?raw',
  import: 'default',
  eager: true
});

const props = defineProps<{ name: string; alt: string }>();

const svg = computed(() => {
  const source = sources[`./diagrams/${props.name}.svg`];

  // A name with no drawing behind it fails the build rather than leaving a
  // blank where the diagram was.
  if (!source) {
    throw new Error(`There is no diagram named ${props.name}.`);
  }

  return source;
});
</script>

<template>
  <figure class="diagram" role="img" :aria-label="alt">
    <div class="diagram-body" aria-hidden="true" v-html="svg" />
  </figure>
</template>
