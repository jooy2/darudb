/// The complex suite on DaruDB through its Dart package, as an application
/// uses it: classes the generator reads, queries that run many times
/// prepared once, and an update that sets the one field it changes.
library;

import 'package:darudb/darudb.dart';

import 'common.dart' show Digest, Rows;
import 'complex.dart';
import 'complex_model.dart';

const _schema = Schema(1, [organizationSchema, personSchema, postSchema]);

void _person(Digest d, Person p) => d.add(p.id!, p.age);

void _post(Digest d, Post p) => d.add(p.id!, p.likes);

void run(String directory, Rows rows) {
  final db = Database.open('$directory/complex.darudb', schema: _schema);

  // The objects are made before the load's time starts.
  final organizations = [
    for (var n = 0; n < organizationCount; n++)
      () {
        final o = organization(n);

        return Organization(
          code: o.code,
          name: o.name,
          kind: o.kind,
          country: o.country,
          founded: o.founded,
        );
      }(),
  ];
  final people = [
    for (var n = 0; n < peopleCount; n++)
      () {
        final p = person(n);

        return Person(
          name: p.name,
          email: p.email,
          age: p.age,
          country: p.country,
          address: Address(city: p.city, zip: p.zip),
          score: p.score,
          active: p.active,
          joinedAt: p.joinedAt,
          organization: Link<Organization>(p.organization),
          tags: p.tags,
        );
      }(),
  ];
  final posts = [
    for (var m = 0; m < postCount; m++)
      () {
        final p = post(m);

        return Post(
          author: Link<Person>(p.author),
          title: p.title,
          body: p.body,
          likes: p.likes,
          language: p.language,
          createdAt: p.createdAt,
          pinned: p.pinned,
          tags: p.tags,
        );
      }(),
  ];

  rows.all(ComplexRows.load.$1, ComplexRows.load.$2, (_) {
    for (final part in batches(organizations)) {
      db.write((txn) => txn.collection(organizationSchema).insertMany(part));
    }

    for (final part in batches(people)) {
      db.write((txn) => txn.collection(personSchema).insertMany(part));
    }

    for (final part in batches(posts)) {
      db.write((txn) => txn.collection(postSchema).insertMany(part));
    }
  });

  final newest = db.prepare(
    postSchema,
    r'author == $0 SORT BY createdAt DESC LIMIT 5',
  );
  final filter = db.prepare(
    personSchema,
    r'age BETWEEN $0 AND $1 AND country == $2 AND active == true '
    'SORT BY score DESC LIMIT 50',
  );
  final either = db.prepare(personSchema, r'country IN [$0, $1] OR age >= 78');
  final prefix = db.prepare(personSchema, r'name STARTSWITH $0');
  final authorCountry = db.prepare(
    postSchema,
    r'author.country == $0 AND likes >= 900 SORT BY createdAt DESC LIMIT 20',
  );
  final twoLinks = db.prepare(
    postSchema,
    r'author.organization.kind == $0 AND language == $1',
  );
  final tag = db.prepare(
    postSchema,
    r'tags CONTAINS $0 SORT BY likes DESC LIMIT 20',
  );
  final text = db.prepare(postSchema, r'body CONTAINS $0');
  final likes = db.prepare(postSchema, r'likes >= 500 AND language == $0');
  final inCountry = db.prepare(personSchema, r'country == $0');
  final byAuthor = db.prepare(postSchema, r'author == $0');

  db.read((txn) {
    final organizations = txn.collection(organizationSchema);
    final people = txn.collection(personSchema);
    final posts = txn.collection(postSchema);

    rows.each(ComplexRows.detail.$1, ComplexRows.detail.$2, (round, d) {
      final found = people.get(Runs.detail(round));

      if (found == null) {
        return;
      }

      _person(d, found);

      final organization = organizations.get(found.organization.key as String);

      if (organization != null) {
        d.add(organization.founded, 0);
      }

      for (final p in posts.findPrepared(newest, [found.id])) {
        _post(d, p);
      }
    });
    rows.each(ComplexRows.filter.$1, ComplexRows.filter.$2, (round, d) {
      final (age, country) = Runs.filter(round);

      for (final p in people.findPrepared(filter, [age, age + 9, country])) {
        _person(d, p);
      }
    });
    rows.each(ComplexRows.either.$1, ComplexRows.either.$2, (round, d) {
      final (first, second) = Runs.either(round);

      d.number(people.countPrepared(either, [first, second]));
    });
    rows.each(ComplexRows.prefix.$1, ComplexRows.prefix.$2, (round, d) {
      for (final p in people.findPrepared(prefix, [Runs.prefix(round)])) {
        _person(d, p);
      }
    });
    rows.each(ComplexRows.page.$1, ComplexRows.page.$2, (round, d) {
      final page = people.find(
        (q) => q.sortBy(q.joinedAt).offset(Runs.offset(round)).limit(50),
      );

      for (final p in page) {
        _person(d, p);
      }
    });
    rows.each(ComplexRows.authorCountry.$1, ComplexRows.authorCountry.$2, (
      round,
      d,
    ) {
      for (final p in posts.findPrepared(authorCountry, [
        Runs.authorCountry(round),
      ])) {
        _post(d, p);
      }
    });
    rows.each(ComplexRows.twoLinks.$1, ComplexRows.twoLinks.$2, (round, d) {
      final (kind, language) = Runs.twoLinks(round);

      d.number(posts.countPrepared(twoLinks, [kind, language]));
    });
    rows.each(ComplexRows.tag.$1, ComplexRows.tag.$2, (round, d) {
      for (final p in posts.findPrepared(tag, [Runs.tag(round)])) {
        _post(d, p);
      }
    });
    rows.each(ComplexRows.text.$1, ComplexRows.text.$2, (round, d) {
      d.number(posts.countPrepared(text, [Runs.phrase(round)]));
    });
    rows.each(ComplexRows.count.$1, ComplexRows.count.$2, (round, d) {
      d.number(posts.countPrepared(likes, [Runs.language(round)]));
    });
  });

  rows.all(ComplexRows.update, updatedCount(), (d) {
    db.write((txn) {
      final people = txn.collection(personSchema);

      for (final country in Runs.updated) {
        final found = people.findPrepared(inCountry, [country]);

        d.number(found.length);

        for (final p in found) {
          people.update(p.id!, (q) => [q.age.set((p.age + 1) % 80)]);
        }
      }
    });
  });
  rows.all(ComplexRows.delete, Runs.deletedCount, (d) {
    db.write((txn) {
      final people = txn.collection(personSchema);
      final posts = txn.collection(postSchema);

      for (var round = 0; round < Runs.deletedCount; round++) {
        final id = Runs.deleted(round);
        var deleted = 0;

        for (final p in posts.findPrepared(byAuthor, [id])) {
          if (posts.delete(p.id!)) {
            deleted++;
          }
        }

        if (people.delete(id)) {
          d.add(id, deleted);
        }
      }
    });
  });

  final leftPeople = Digest();
  final leftPosts = Digest();

  db.read((txn) {
    for (final p in txn.collection(personSchema).find()) {
      _person(leftPeople, p);
    }

    for (final p in txn.collection(postSchema).find()) {
      _post(leftPosts, p);
    }
  });
  rows.check('left-people', leftPeople);
  rows.check('left-posts', leftPosts);
  db.close();
}
