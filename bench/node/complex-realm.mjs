// The complex suite on Realm through its JavaScript SDK, for applications
// that move from it. Links are object properties, the address is an embedded
// object, and tags are a list of strings, which Realm does not index. A link
// cannot be indexed either, so a person's posts are found through the
// backlink Realm offers for that, `posts` on each person. Realm
// assigns no keys, so each object gets the key the other stores assign, and a
// link is set to the object it points at. Queries are Realm Query Language
// strings; sorting by `id` as well gives the order the other stores' results
// come in, and a page is a slice of the sorted results, since the language
// has no offset. An update sets the one property.
import Realm from 'realm';
import { join } from 'node:path';

import { Digest, Rows, finish } from './common.mjs';
import { ROWS, batches, objects, runs, updatedCount } from './complex.mjs';

const [, , directory] = process.argv;
const ORGANIZATION = {
  name: 'Organization',
  primaryKey: 'code',
  properties: {
    code: 'string',
    name: 'string',
    kind: { type: 'string', indexed: true },
    country: { type: 'string', indexed: true },
    founded: 'int'
  }
};
const ADDRESS = {
  name: 'Address',
  embedded: true,
  properties: { city: 'string', zip: 'string' }
};
const PERSON = {
  name: 'Person',
  primaryKey: 'id',
  properties: {
    id: 'int',
    name: 'string',
    email: { type: 'string', indexed: true },
    age: { type: 'int', indexed: true },
    country: { type: 'string', indexed: true },
    address: 'Address',
    score: 'double',
    active: 'bool',
    joinedAt: { type: 'int', indexed: true },
    organization: 'Organization',
    tags: 'string[]',
    posts: { type: 'linkingObjects', objectType: 'Post', property: 'author' }
  }
};
const POST = {
  name: 'Post',
  primaryKey: 'id',
  properties: {
    id: 'int',
    author: 'Person',
    title: 'string',
    body: 'string',
    likes: { type: 'int', indexed: true },
    language: { type: 'string', indexed: true },
    createdAt: { type: 'int', indexed: true },
    pinned: 'bool',
    tags: 'string[]'
  }
};

const realm = new Realm({
  path: join(directory, 'complex.realm'),
  schema: [ORGANIZATION, ADDRESS, PERSON, POST]
});
const rows = new Rows();
const made = objects();

/** A person as the other stores read one: plain values, the link a key. */
const personOf = (p) => ({
  id: p.id,
  name: p.name,
  email: p.email,
  age: p.age,
  country: p.country,
  address: { city: p.address.city, zip: p.address.zip },
  score: p.score,
  active: p.active,
  joinedAt: p.joinedAt,
  organization: p.organization?.code ?? null,
  tags: [...p.tags]
});

const postOf = (p) => ({
  id: p.id,
  author: p.author?.id ?? null,
  title: p.title,
  body: p.body,
  likes: p.likes,
  language: p.language,
  createdAt: p.createdAt,
  pinned: p.pinned,
  tags: [...p.tags]
});

const person = (d, p) => d.add(p.id, p.age);
const post = (d, p) => d.add(p.id, p.likes);

rows.all(...ROWS.load, () => {
  const organizations = new Map();
  const people = [];
  let id = 0;

  for (const batch of batches(made.organizations)) {
    realm.write(() => {
      for (const o of batch) organizations.set(o.code, realm.create('Organization', o));
    });
  }

  for (const batch of batches(made.people)) {
    realm.write(() => {
      for (const { city, zip, organization, ...p } of batch) {
        people.push(
          realm.create('Person', {
            ...p,
            id: ++id,
            address: { city, zip },
            organization: organizations.get(organization)
          })
        );
      }
    });
  }

  id = 0;

  for (const batch of batches(made.posts)) {
    realm.write(() => {
      for (const { author, ...p } of batch) {
        realm.create('Post', { ...p, id: ++id, author: people[author - 1] });
      }
    });
  }
});

const people = realm.objects('Person');
const posts = realm.objects('Post');

rows.each(...ROWS.detail, (round, d) => {
  const found = realm.objectForPrimaryKey('Person', runs.detail(round));

  if (!found) return;

  person(d, personOf(found));

  const organization = found.organization;

  if (organization) d.add(organization.founded, 0);

  const newest = found.posts.sorted([
    ['createdAt', true],
    ['id', false]
  ]);

  for (const p of newest.slice(0, 5)) post(d, postOf(p));
});
rows.each(...ROWS.filter, (round, d) => {
  const [age, country] = runs.filter(round);
  const found = people.filtered(
    'age BETWEEN {$0, $1} AND country == $2 AND active == true SORT(score DESC, id ASC) LIMIT(50)',
    age,
    age + 9,
    country
  );

  for (const p of found) person(d, personOf(p));
});
rows.each(...ROWS.either, (round, d) => {
  d.number(people.filtered('country IN {$0, $1} OR age >= 78', ...runs.either(round)).length);
});
rows.each(...ROWS.prefix, (round, d) => {
  for (const p of people.filtered('name BEGINSWITH $0 SORT(id ASC)', runs.prefix(round))) {
    person(d, personOf(p));
  }
});

const byJoined = people.sorted([
  ['joinedAt', false],
  ['id', false]
]);

rows.each(...ROWS.page, (round, d) => {
  const offset = runs.offset(round);

  for (const p of byJoined.slice(offset, offset + 50)) person(d, personOf(p));
});
rows.each(...ROWS.authorCountry, (round, d) => {
  const found = posts.filtered(
    'author.country == $0 AND likes >= 900 SORT(createdAt DESC, id ASC) LIMIT(20)',
    runs.authorCountry(round)
  );

  for (const p of found) post(d, postOf(p));
});
rows.each(...ROWS.twoLinks, (round, d) => {
  d.number(
    posts.filtered('author.organization.kind == $0 AND language == $1', ...runs.twoLinks(round))
      .length
  );
});
rows.each(...ROWS.tag, (round, d) => {
  const found = posts.filtered('ANY tags == $0 SORT(likes DESC, id ASC) LIMIT(20)', runs.tag(round));

  for (const p of found) post(d, postOf(p));
});
rows.each(...ROWS.text, (round, d) => {
  d.number(posts.filtered('body CONTAINS $0', runs.phrase(round)).length);
});
rows.each(...ROWS.count, (round, d) => {
  d.number(posts.filtered('likes >= 500 AND language == $0', runs.language(round)).length);
});

rows.all(ROWS.update, updatedCount(), (d) => {
  realm.write(() => {
    for (const country of runs.updated) {
      const found = people.filtered('country == $0', country).snapshot();

      d.number(found.length);

      for (const p of found) p.age = (p.age + 1) % 80;
    }
  });
});
rows.all(ROWS.delete, runs.deletedCount, (d) => {
  realm.write(() => {
    for (let round = 0; round < runs.deletedCount; round++) {
      const id = runs.deleted(round);
      const found = realm.objectForPrimaryKey('Person', id);

      if (!found) continue;

      const written = found.posts;
      const deleted = written.length;

      realm.delete(written);
      realm.delete(found);
      d.add(id, deleted);
    }
  });
});

const leftPeople = new Digest();
const leftPosts = new Digest();

for (const p of people.sorted('id')) person(leftPeople, personOf(p));
for (const p of posts.sorted('id')) post(leftPosts, postOf(p));
rows.check('left-people', leftPeople);
rows.check('left-posts', leftPosts);
realm.close();
finish(rows);
