// What every store runs in the complex suite: three collections that link to
// each other, the objects in them, and the parameters of each row's runs.
// bench/README.md describes the suite, and the other languages' harnesses
// draw the same objects and parameters from the same hash.

export const ORGANIZATIONS = 1_000;
export const PEOPLE = 100_000;
export const POSTS = 300_000;

/** Objects a write transaction of the load inserts. */
export const BATCH = 10_000;

export const KINDS = ['company', 'school', 'hospital', 'agency', 'club'];

export const COUNTRIES = [
  'kr', 'us', 'jp', 'de', 'fr', 'gb', 'cn', 'in', 'br', 'ca',
  'au', 'es', 'it', 'nl', 'se', 'mx', 'id', 'tr', 'pl', 'vn'
];

export const LANGUAGES = ['en', 'ko', 'ja', 'de', 'fr', 'es'];

export const TAGS = [
  'travel', 'food', 'music', 'sport', 'news', 'tech', 'art', 'books', 'games', 'film',
  'health', 'science', 'nature', 'design', 'fashion', 'history', 'photo', 'pets', 'cars', 'money'
];

export const WORDS = [
  'able', 'about', 'above', 'across', 'after', 'again', 'air', 'almost', 'along', 'always',
  'animal', 'answer', 'around', 'away', 'back', 'before', 'began', 'below', 'between', 'black',
  'body', 'book', 'both', 'bring', 'build', 'call', 'came', 'carry', 'change', 'child', 'city',
  'close', 'color', 'come', 'country', 'cover', 'cross', 'dark', 'day', 'deep', 'door', 'draw',
  'early', 'earth', 'east', 'end', 'even', 'every', 'face', 'fall', 'family', 'far', 'farm',
  'fast', 'field', 'fire', 'first', 'food', 'foot', 'form', 'found', 'free', 'friend', 'full'
];

/** A 32-bit integer hash, the same in every language's harness. */
const hash = (value) => {
  let x = Math.imul((value >>> 16) ^ value, 0x045d9f3b) >>> 0;

  x = Math.imul((x >>> 16) ^ x, 0x045d9f3b) >>> 0;

  return ((x >>> 16) ^ x) >>> 0;
};

/** The value `salt` draws for the `n`th object or run. */
export const mix = (n, salt) => hash((hash(n >>> 0) + salt) >>> 0);

const pick = (values, n, salt) => values[mix(n, salt) % values.length];

const tagsOf = (count, first) =>
  Array.from({ length: count }, (_, at) => TAGS[((first % 20) + at * 7) % 20]);

/** The code of the `n`th organization. */
export const code = (n) => `org-${String(n).padStart(4, '0')}`;

export const organization = (n) => ({
  code: code(n),
  name: `organization ${n}`,
  kind: pick(KINDS, n, 1),
  country: pick(COUNTRIES, n, 2),
  founded: 1900 + (mix(n, 3) % 120)
});

/** The `n`th person, counting from 0. They get the key `n + 1`. */
export const person = (n) => ({
  name: `person ${n}`,
  email: `${n}@example.com`,
  age: mix(n, 1) % 80,
  country: pick(COUNTRIES, n, 2),
  city: `city ${mix(n, 3) % 500}`,
  zip: String(mix(n, 4) % 100_000).padStart(5, '0'),
  score: (mix(n, 5) % 1_000_000) / 1_000_000,
  active: mix(n, 6) % 7 !== 0,
  // A permutation of the people, so that no two join at once.
  joinedAt: 1_600_000_000 + ((n * 7_907) % PEOPLE) * 37,
  organization: code(mix(n, 7) % ORGANIZATIONS),
  tags: tagsOf(1 + (mix(n, 8) % 3), mix(n, 9))
});

/** The `m`th post, counting from 0. It gets the key `m + 1`. */
export const post = (m) => ({
  author: 1 + (mix(m, 1) % PEOPLE),
  title: `post ${m}`,
  body: Array.from({ length: 12 }, (_, at) => WORDS[mix(m, 100 + at) % 64]).join(' '),
  likes: mix(m, 3) % 1_000,
  language: pick(LANGUAGES, m, 4),
  // A permutation of the posts, so that no two are made at once.
  createdAt: 1_650_000_000 + ((m * 104_729) % POSTS) * 60,
  pinned: mix(m, 7) % 50 === 0,
  tags: tagsOf(mix(m, 5) % 3, mix(m, 6))
});

/** The parameters of each row's runs, counting runs from 0. */
export const runs = {
  detail: (round) => 1 + (mix(round, 11) % PEOPLE),
  filter: (round) => [mix(round, 12) % 70, COUNTRIES[mix(round, 13) % 20]],
  either: (round) => [COUNTRIES[round % 20], COUNTRIES[(round + 7) % 20]],
  prefix: (round) => `person ${mix(round, 14) % 1_000}`,
  offset: (round) => round * 997,
  authorCountry: (round) => COUNTRIES[round % 20],
  twoLinks: (round) => [KINDS[round % 5], LANGUAGES[round % 6]],
  tag: (round) => TAGS[round % 20],
  phrase: (round) => `${WORDS[mix(round, 15) % 64]} ${WORDS[mix(round, 16) % 64]}`,
  language: (round) => LANGUAGES[round % 6],
  /** The countries whose people turn a year older. */
  updated: ['kr', 'us', 'jp', 'de'],
  /** The people deleted with their posts. */
  deleted: (round) => 1 + round * 97,
  deletedCount: 1_000
};

/** How many people `update-query` changes, which its time is divided by. */
export const updatedCount = () => {
  let count = 0;

  for (let n = 0; n < PEOPLE; n++) {
    if (runs.updated.includes(pick(COUNTRIES, n, 2))) count++;
  }

  return count;
};

/** The rows of the suite, with how many runs or objects each times. */
export const ROWS = {
  load: ['load', ORGANIZATIONS + PEOPLE + POSTS],
  detail: ['person-detail', 10_000],
  filter: ['people-filter', 500],
  either: ['people-or', 100],
  prefix: ['people-prefix', 100],
  page: ['people-page', 100],
  authorCountry: ['posts-author-country', 100],
  twoLinks: ['posts-two-links', 30],
  tag: ['posts-tag', 200],
  text: ['posts-text', 10],
  count: ['posts-count', 200],
  update: 'update-query',
  delete: 'delete-cascade'
};

/** Every object of the load, made before its time starts. */
export const objects = () => ({
  organizations: Array.from({ length: ORGANIZATIONS }, (_, n) => organization(n)),
  people: Array.from({ length: PEOPLE }, (_, n) => person(n)),
  posts: Array.from({ length: POSTS }, (_, m) => post(m))
});

/** Splits `items` into arrays of `BATCH`. */
export const batches = (items) =>
  Array.from({ length: Math.ceil(items.length / BATCH) }, (_, at) =>
    items.slice(at * BATCH, (at + 1) * BATCH)
  );
