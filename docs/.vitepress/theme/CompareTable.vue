<script setup lang="ts">
import { useData } from 'vitepress';
import { computed, ref } from 'vue';
import { CODE_LANGUAGES } from '../languages';
import { localeOf } from './i18n';
import { languagesOf, PRODUCTS, SECTIONS, type Level } from './comparison';

/*
 * The feature table of the comparison page, drawn from `comparison.ts`.
 *
 * A row per feature and a column per database, each cell marked supported,
 * partly supported or not, with a short note where the mark alone would
 * mislead. The marks are shapes as well as colours, and each carries its word
 * for a screen reader, so colour is never the only thing that says it.
 *
 * The reader's language decides what is dimmed: a database without a package
 * for it is drawn faded, by CSS from `<html data-code-lang>` as everything else
 * on the site that follows the language switch, so it is right before the app
 * has started. The checkbox hides those columns instead.
 */

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));

const onlyMine = ref(false);

const STRINGS = {
  en: {
    feature: 'Feature',
    only: 'Only the databases with a package for',
    region: 'Feature comparison, scrolls sideways',
    unavailable: 'No package for',
    yes: 'Supported',
    partial: 'Partly',
    no: 'Not supported',
    legend: 'Marks'
  },
  ko: {
    feature: '기능',
    only: '다음 언어용 패키지가 있는 DB만 보기:',
    region: '기능 비교 표, 옆으로 스크롤됩니다',
    unavailable: '패키지 없음:',
    yes: '지원',
    partial: '일부 지원',
    no: '지원 안 함',
    legend: '표시'
  }
};

const text = computed(() => STRINGS[locale.value]);

/** A product's languages, as the space-separated list `data-langs` holds. */
const langsOf = (id: string) => languagesOf(id).join(' ');

/** The languages a product has no package for, each with its name. */
const missing = (id: string) =>
  CODE_LANGUAGES.filter((language) => !languagesOf(id).includes(language.id));

const marked: Level[] = ['yes', 'partial', 'no'];
</script>

<template>
  <div class="compare vp-raw">
    <div class="compare-controls">
      <label class="compare-only">
        <input v-model="onlyMine" type="checkbox" />
        <span
          >{{ text.only }}
          <strong
            v-for="language in CODE_LANGUAGES"
            :key="language.id"
            class="lang-only"
            :data-code-lang="language.id"
            >{{ language.label }}</strong
          ></span
        >
      </label>

      <ul class="compare-legend" :aria-label="text.legend">
        <li v-for="level in marked" :key="level" :data-level="level">
          <span class="compare-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24" focusable="false">
              <path v-if="level === 'yes'" d="m5 12.5 4.5 4.5L19 7" />
              <path v-else-if="level === 'partial'" d="M6 12h12" />
              <path v-else d="M7 7l10 10M17 7 7 17" />
            </svg>
          </span>
          {{ text[level] }}
        </li>
      </ul>
    </div>

    <div class="compare-scroll" role="region" :aria-label="text.region" tabindex="0">
      <table class="compare-table" :class="{ 'compare-only-mine': onlyMine }">
        <thead>
          <tr>
            <th scope="col" class="compare-corner">{{ text.feature }}</th>
            <th
              v-for="product in PRODUCTS"
              :key="product.id"
              scope="col"
              class="compare-product"
              :class="{ 'compare-self': product.self }"
              :data-langs="langsOf(product.id)"
            >
              <a v-if="product.site" :href="product.site" target="_blank" rel="noopener">{{
                product.name
              }}</a>
              <template v-else>{{ product.name }}</template>
              <span
                v-for="language in missing(product.id)"
                :key="language.id"
                class="lang-only compare-unavailable"
                :data-code-lang="language.id"
                >{{ text.unavailable }} {{ language.label }}</span
              >
            </th>
          </tr>
        </thead>
        <tbody v-for="section in SECTIONS" :key="section.id">
          <tr class="compare-section">
            <th scope="colgroup" :colspan="PRODUCTS.length + 1">
              <span class="compare-section-label">{{ section.label[locale] }}</span>
            </th>
          </tr>
          <tr v-for="row in section.rows" :key="row.id" :data-row-lang="row.language">
            <th scope="row" class="compare-feature">{{ row.label[locale] }}</th>
            <td
              v-for="product in PRODUCTS"
              :key="product.id"
              :class="{ 'compare-self': product.self }"
              :data-langs="langsOf(product.id)"
              :data-level="row.cells[product.id].level"
            >
              <span
                v-if="row.cells[product.id].level !== 'text'"
                class="compare-mark"
                aria-hidden="true"
              >
                <svg viewBox="0 0 24 24" focusable="false">
                  <path v-if="row.cells[product.id].level === 'yes'" d="m5 12.5 4.5 4.5L19 7" />
                  <path v-else-if="row.cells[product.id].level === 'partial'" d="M6 12h12" />
                  <path v-else d="M7 7l10 10M17 7 7 17" />
                </svg>
              </span>
              <span v-if="row.cells[product.id].level !== 'text'" class="compare-level">{{
                text[row.cells[product.id].level as 'yes' | 'partial' | 'no']
              }}</span>
              <span v-if="row.cells[product.id].note" class="compare-note">{{
                row.cells[product.id].note![locale]
              }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
