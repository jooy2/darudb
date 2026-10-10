<script setup lang="ts">
import { useData } from 'vitepress';
import { computed } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import { localeOf } from './i18n';
import data from './performance.json';
import { ROWS, STORES, TEXT, type Data, type Results } from './performance';

/*
 * The benchmark's results, one table for each language, of which CSS shows
 * the reader's, as a `::: lang` block does.
 *
 * Each cell is the median time of one operation, and below it, for a store
 * other than DaruDB, how many times as long as DaruDB that store takes. The
 * fastest time of a row is set in bold.
 */

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));
const text = computed(() => TEXT[locale.value]);
const results = (data as Data).languages;

/** `538 ns`, `8.68 µs`, `5.35 ms`. */
function time(ns: number): string {
  if (ns >= 1e6) return `${(ns / 1e6).toFixed(2)} ms`;
  if (ns >= 1e3) return `${(ns / 1e3).toFixed(2)} µs`;

  return `${Math.round(ns)} ns`;
}

function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => String(values[key] ?? ''));
}

function table(result: Results) {
  const stores = result.stores.map((store) => store.id);
  const rows = ROWS.flatMap((row) => {
    const found = result.rows.find((measured) => measured.id === row.id);

    if (!found) return [];

    const daru = found.results.daru?.median ?? 0;
    const fastest = Math.min(...stores.map((store) => found.results[store]?.median ?? Infinity));

    return [
      {
        id: row.id,
        label: row.label[locale.value],
        cells: stores.map((store) => {
          // A store with no way to do the row, such as a deferred commit in a
          // store whose every commit syncs, gets an empty cell.
          if (!found.results[store]) {
            return {
              store,
              none: true,
              time: '—',
              fastest: false,
              ratio: null,
              standing: '',
              title: fill(text.value.none, { store: STORES[store] })
            };
          }

          const median = found.results[store].median;
          const ratio = store === 'daru' || daru === 0 ? null : median / daru;

          return {
            store,
            none: false,
            time: time(median),
            fastest: median === fastest,
            ratio: ratio === null ? null : ratio.toFixed(2),
            standing:
              ratio === null ? '' : ratio > 1.05 ? 'behind' : ratio < 0.95 ? 'ahead' : 'even',
            title:
              ratio === null
                ? ''
                : fill(text.value.ratioTitle, { store: STORES[store], ratio: ratio.toFixed(2) })
          };
        })
      }
    ];
  });

  return {
    stores,
    rows,
    measured: fill(text.value.measured, {
      cpu: result.machine.cpu,
      cores: result.machine.cores,
      os: result.machine.os,
      date: result.generated.slice(0, 10),
      runs: result.runs
    }),
    versions: result.stores.map((store) => `${STORES[store.id]} ${store.version}`).join(', ')
  };
}

const languages = computed(() =>
  CODE_LANGUAGES.map((language) => {
    const result = results[language.id];

    return { id: language.id, table: result ? table(result) : null };
  })
);
</script>

<template>
  <div
    v-for="language in languages"
    :key="language.id"
    class="lang-only performance"
    :data-code-lang="language.id"
  >
    <template v-if="language.table">
      <p class="performance-meta">{{ language.table.measured }} {{ text.ratio }}</p>
      <div class="performance-scroll">
        <table class="performance-table">
          <thead>
            <tr>
              <th scope="col">{{ text.workload }}</th>
              <th
                v-for="store in language.table.stores"
                :key="store"
                scope="col"
                :class="{ 'performance-self': store === 'daru' }"
              >
                {{ STORES[store] }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in language.table.rows" :key="row.id">
              <th scope="row">{{ row.label }}</th>
              <td
                v-for="cell in row.cells"
                :key="cell.store"
                :class="{ 'performance-self': cell.store === 'daru' }"
              >
                <span v-if="cell.none" :title="cell.title"
                  ><span class="performance-time" aria-hidden="true">{{ cell.time }}</span
                  ><span class="visually-hidden">{{ cell.title }}</span></span
                >
                <span
                  v-else
                  class="performance-time"
                  :class="{ 'performance-fastest': cell.fastest }"
                  >{{ cell.time }}</span
                >
                <span
                  v-if="cell.ratio"
                  class="performance-ratio"
                  :class="`performance-${cell.standing}`"
                  :title="cell.title"
                  >×{{ cell.ratio }}</span
                >
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <p class="performance-versions">{{ text.versions }}: {{ language.table.versions }}</p>
    </template>
    <p v-else class="performance-meta">{{ text.missing }}</p>
  </div>
</template>
