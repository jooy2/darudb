<script setup lang="ts">
import { useData } from 'vitepress';
import { computed, ref } from 'vue';
import type { LanguageId } from '../languages';
import { localeOf, t } from './i18n';

/*
 * The data example on the home page: a few objects of a `users` collection and
 * a handful of queries, each in the query language and as each language's
 * builder writes it. Picking one marks the objects it finds, in the order the
 * engine returns them.
 *
 * Nothing here runs the engine. Each query's meaning is written out as a
 * function beside its text, following the rules the guide states: without a
 * sort, objects come in primary key order; a condition on a list holds when it
 * holds for an element; null meets no condition but the test for null. The
 * objects are few enough that each answer can be checked by eye.
 *
 * Every language's builder is in the document and CSS shows the reader's, as a
 * `::: lang` block does.
 */

interface User {
  id: number;
  name: string;
  age: number;
  city: string;
  tags: string[];
  email: string | null;
}

interface Example {
  text: string;
  code: Record<LanguageId, string>;
  test: (user: User) => boolean;
  sort?: (a: User, b: User) => number;
  limit?: number;
}

const users: User[] = [
  { id: 1, name: 'Ada', age: 36, city: 'Seoul', tags: ['admin'], email: 'ada@example.com' },
  { id: 2, name: 'Ben', age: 24, city: 'Busan', tags: [], email: null },
  { id: 3, name: 'Chloe', age: 29, city: 'Lisbon', tags: ['editor'], email: 'chloe@example.com' },
  {
    id: 4,
    name: 'Jin',
    age: 41,
    city: 'Seoul',
    tags: ['admin', 'editor'],
    email: 'jin@example.com'
  },
  { id: 5, name: 'Jules', age: 19, city: 'Lisbon', tags: [], email: null },
  { id: 6, name: 'Mina', age: 33, city: 'Seoul', tags: ['editor'], email: 'mina@example.com' },
  { id: 7, name: 'Noah', age: 27, city: 'Busan', tags: ['admin'], email: 'noah@example.com' },
  { id: 8, name: 'Sora', age: 22, city: 'Seoul', tags: [], email: 'sora@example.com' }
];

const examples: Example[] = [
  {
    text: 'age >= 30',
    code: {
      rust: 'Query::new().filter(Filter::ge("age", 30))',
      node: "(q) => q.where('age', '>=', 30)",
      dart: '(q) => q.where(q.age.atLeast(30))',
      python: 'F.age >= 30'
    },
    test: (user) => user.age >= 30
  },
  {
    text: 'city == "Seoul" AND age < 40',
    code: {
      rust: 'Query::new().filter(Filter::eq("city", "Seoul").and(Filter::lt("age", 40)))',
      node: "(q) => q.where('city', '==', 'Seoul').where('age', '<', 40)",
      dart: "(q) => q.where(q.city.equals('Seoul') & q.age.lessThan(40))",
      python: '(F.city == "Seoul") & (F.age < 40)'
    },
    test: (user) => user.city === 'Seoul' && user.age < 40
  },
  {
    text: 'name STARTSWITH "J"',
    code: {
      rust: 'Query::new().filter(Filter::starts_with("name", "J"))',
      node: "(q) => q.where('name', 'startsWith', 'J')",
      dart: "(q) => q.where(q.name.startsWith('J'))",
      python: 'F.name.startswith("J")'
    },
    test: (user) => user.name.startsWith('J')
  },
  {
    text: 'tags CONTAINS "admin"',
    code: {
      rust: 'Query::new().filter(Filter::contains("tags", "admin"))',
      node: "(q) => q.where('tags', 'contains', 'admin')",
      dart: "(q) => q.where(q.tags.contains('admin'))",
      python: 'F.tags.contains("admin")'
    },
    test: (user) => user.tags.includes('admin')
  },
  {
    text: 'email IS NULL',
    code: {
      rust: 'Query::new().filter(Filter::is_null("email"))',
      node: "(q) => q.where('email', '==', null)",
      dart: '(q) => q.where(q.email.isNull())',
      python: 'F.email.is_null()'
    },
    test: (user) => user.email === null
  },
  {
    text: 'age BETWEEN 20 AND 29 SORT BY age DESC',
    code: {
      rust: 'Query::new().filter(Filter::between("age", 20, 29)).sort_by_desc("age")',
      node: "(q) => q.where('age', 'between', [20, 29]).sortBy('age', 'desc')",
      dart: '(q) => q.where(q.age.between(20, 29)).sortBy(q.age, descending: true)',
      python: 'where(F.age.between(20, 29)).sort_by(F.age, descending=True)'
    },
    test: (user) => user.age >= 20 && user.age <= 29,
    sort: (a, b) => b.age - a.age
  },
  {
    text: 'city IN ["Busan", "Lisbon"] SORT BY name LIMIT 2',
    code: {
      rust: 'Query::new().filter(Filter::is_in("city", ["Busan", "Lisbon"])).sort_by("name").limit(2)',
      node: "(q) => q.where('city', 'in', ['Busan', 'Lisbon']).sortBy('name').limit(2)",
      dart: "(q) => q.where(q.city.isIn(['Busan', 'Lisbon'])).sortBy(q.name).limit(2)",
      python: 'where(F.city.is_in(["Busan", "Lisbon"])).sort_by(F.name).limit(2)'
    },
    test: (user) => user.city === 'Busan' || user.city === 'Lisbon',
    sort: (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
    limit: 2
  }
];

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));
const chosen = ref(0);
const example = computed(() => examples[chosen.value]);

/** Each found object's place in the result, by its id. */
const places = computed(() => {
  const { test, sort, limit } = example.value;
  // Primary key order first, which a sort keeps among objects it finds equal.
  const found = users.filter(test).sort((a, b) => a.id - b.id);

  if (sort) {
    found.sort((a, b) => sort(a, b) || a.id - b.id);
  }

  return new Map(found.slice(0, limit ?? found.length).map((user, index) => [user.id, index + 1]));
});

const found = computed(() =>
  t(locale.value, 'demoFound')
    .replace('{count}', String(places.value.size))
    .replace('{total}', String(users.length))
);

const quoted = (text: string) => JSON.stringify(text);
</script>

<template>
  <div class="query-demo">
    <div class="query-demo-picks" role="group" :aria-label="t(locale, 'demoQueries')">
      <button
        v-for="(item, index) in examples"
        :key="item.text"
        type="button"
        class="query-demo-pick"
        :aria-pressed="index === chosen"
        @click="chosen = index"
      >
        {{ item.text }}
      </button>
    </div>

    <div class="query-demo-panel">
      <dl class="query-demo-forms">
        <div class="query-demo-form">
          <dt>{{ t(locale, 'demoText') }}</dt>
          <dd>
            <code>{{ example.text }}</code>
          </dd>
        </div>
        <div class="query-demo-form">
          <dt>{{ t(locale, 'demoCode') }}</dt>
          <dd>
            <code
              v-for="(code, id) in example.code"
              :key="id"
              class="lang-only"
              :data-code-lang="id"
              >{{ code }}</code
            >
          </dd>
        </div>
      </dl>

      <div class="query-demo-table">
        <table>
          <caption>{{ t(locale, 'demoCaption') }}</caption>
          <thead>
            <tr>
              <th scope="col">#</th>
              <th scope="col">id</th>
              <th scope="col">name</th>
              <th scope="col">age</th>
              <th scope="col">city</th>
              <th scope="col">tags</th>
              <th scope="col">email</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="user in users"
              :key="user.id"
              :class="places.has(user.id) ? 'is-found' : 'is-skipped'"
            >
              <td class="query-demo-place">{{ places.get(user.id) ?? '' }}</td>
              <td>{{ user.id }}</td>
              <td>{{ quoted(user.name) }}</td>
              <td>{{ user.age }}</td>
              <td>{{ quoted(user.city) }}</td>
              <td>[{{ user.tags.map(quoted).join(', ') }}]</td>
              <td :class="{ 'query-demo-null': user.email === null }">
                {{ user.email === null ? 'null' : quoted(user.email) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <p class="query-demo-count" aria-live="polite">{{ found }}</p>
    </div>
  </div>
</template>
