/// The complex suite on Realm through its Dart SDK, for applications that
/// move from it, as the Node.js harness runs it: links are object
/// properties, a person's posts are a backlink, since Realm cannot index a
/// link, and a page is a slice of the sorted results, since Realm Query
/// Language has no offset. Realm assigns no keys, so each object gets the key
/// the other stores assign, and a link is set to the object it points at.
/// Every object read is copied out of Realm into the fields the other stores
/// give, and an update sets the one property.
library;

import 'package:darudb_bench_pinned/complex_model.dart';
import 'package:realm_dart/realm.dart';

// The shared workloads, from the harness beside this one, which this package
// cannot depend on. A file under `bin/` reaches them by their paths.
import '../../../dart/lib/common.dart' show Digest, Rows;
import '../../../dart/lib/complex.dart';

PersonFields _personOf(Person p) => PersonFields(
      id: p.id,
      name: p.name,
      email: p.email,
      age: p.age,
      country: p.country,
      city: p.address!.city,
      zip: p.address!.zip,
      score: p.score,
      active: p.active,
      joinedAt: p.joinedAt,
      organization: p.organization!.code,
      tags: p.tags.toList(),
    );

PostFields _postOf(Post p) => PostFields(
      id: p.id,
      author: p.author!.id,
      title: p.title,
      body: p.body,
      likes: p.likes,
      language: p.language,
      createdAt: p.createdAt,
      pinned: p.pinned,
      tags: p.tags.toList(),
    );

void _person(Digest d, Person p) {
  final copy = _personOf(p);

  d.add(copy.id, copy.age);
}

void _post(Digest d, Post p) {
  final copy = _postOf(p);

  d.add(copy.id, copy.likes);
}

void run(String directory, Rows rows) {
  final realm = Realm(
    Configuration.local([
      Organization.schema,
      Address.schema,
      Person.schema,
      Post.schema,
    ], path: '$directory/complex.realm'),
  );

  // The objects are made before the load's time starts.
  final organizations = [
    for (var n = 0; n < organizationCount; n++) organization(n),
  ];
  final people = [for (var n = 0; n < peopleCount; n++) person(n)];
  final posts = [for (var m = 0; m < postCount; m++) post(m)];

  rows.all(ComplexRows.load.$1, ComplexRows.load.$2, (_) {
    final byCode = <String, Organization>{};
    final added = <Person>[];
    var id = 0;

    for (final part in batches(organizations)) {
      realm.write(() {
        for (final o in part) {
          byCode[o.code] = realm.add(
            Organization(o.code, o.name, o.kind, o.country, o.founded),
          );
        }
      });
    }

    for (final part in batches(people)) {
      realm.write(() {
        for (final p in part) {
          added.add(
            realm.add(
              Person(
                ++id,
                p.name,
                p.email,
                p.age,
                p.country,
                p.score,
                p.active,
                p.joinedAt,
                address: Address(p.city, p.zip),
                organization: byCode[p.organization],
                tags: p.tags,
              ),
            ),
          );
        }
      });
    }

    id = 0;

    for (final part in batches(posts)) {
      realm.write(() {
        for (final p in part) {
          realm.add(
            Post(
              ++id,
              p.title,
              p.body,
              p.likes,
              p.language,
              p.createdAt,
              p.pinned,
              author: added[p.author - 1],
              tags: p.tags,
            ),
          );
        }
      });
    }
  });

  rows.each(ComplexRows.detail.$1, ComplexRows.detail.$2, (round, d) {
    final found = realm.find<Person>(Runs.detail(round));

    if (found == null) {
      return;
    }

    _person(d, found);

    final organization = found.organization;

    if (organization != null) {
      d.add(organization.founded, 0);
    }

    for (final p in found.posts
        .query(
          'TRUEPREDICATE SORT(createdAt DESC, id ASC)',
        )
        .take(5)) {
      _post(d, p);
    }
  });
  rows.each(ComplexRows.filter.$1, ComplexRows.filter.$2, (round, d) {
    final (age, country) = Runs.filter(round);

    for (final p in realm.query<Person>(
      r'age BETWEEN {$0, $1} AND country == $2 AND active == true '
      'SORT(score DESC, id ASC) LIMIT(50)',
      [age, age + 9, country],
    )) {
      _person(d, p);
    }
  });
  rows.each(ComplexRows.either.$1, ComplexRows.either.$2, (round, d) {
    final (first, second) = Runs.either(round);

    d.number(
      realm.query<Person>(r'country IN {$0, $1} OR age >= 78', [
        first,
        second,
      ]).length,
    );
  });
  rows.each(ComplexRows.prefix.$1, ComplexRows.prefix.$2, (round, d) {
    for (final p in realm.query<Person>(r'name BEGINSWITH $0 SORT(id ASC)', [
      Runs.prefix(round),
    ])) {
      _person(d, p);
    }
  });

  final byJoined = realm.query<Person>(
    'TRUEPREDICATE SORT(joinedAt ASC, id ASC)',
  );

  rows.each(ComplexRows.page.$1, ComplexRows.page.$2, (round, d) {
    for (final p in byJoined.skip(Runs.offset(round)).take(50)) {
      _person(d, p);
    }
  });
  rows.each(ComplexRows.authorCountry.$1, ComplexRows.authorCountry.$2, (
    round,
    d,
  ) {
    for (final p in realm.query<Post>(
      r'author.country == $0 AND likes >= 900 '
      'SORT(createdAt DESC, id ASC) LIMIT(20)',
      [Runs.authorCountry(round)],
    )) {
      _post(d, p);
    }
  });
  rows.each(ComplexRows.twoLinks.$1, ComplexRows.twoLinks.$2, (round, d) {
    final (kind, language) = Runs.twoLinks(round);

    d.number(
      realm.query<Post>(
        r'author.organization.kind == $0 AND language == $1',
        [kind, language],
      ).length,
    );
  });
  rows.each(ComplexRows.tag.$1, ComplexRows.tag.$2, (round, d) {
    for (final p in realm.query<Post>(
      r'ANY tags == $0 SORT(likes DESC, id ASC) LIMIT(20)',
      [Runs.tag(round)],
    )) {
      _post(d, p);
    }
  });
  rows.each(ComplexRows.text.$1, ComplexRows.text.$2, (round, d) {
    d.number(
      realm.query<Post>(r'body CONTAINS $0', [Runs.phrase(round)]).length,
    );
  });
  rows.each(ComplexRows.count.$1, ComplexRows.count.$2, (round, d) {
    d.number(
      realm.query<Post>(r'likes >= 500 AND language == $0', [
        Runs.language(round),
      ]).length,
    );
  });

  rows.all(ComplexRows.update, updatedCount(), (d) {
    realm.write(() {
      for (final country in Runs.updated) {
        final found = realm.query<Person>(r'country == $0', [country]).toList();

        d.number(found.length);

        for (final p in found) {
          p.age = (p.age + 1) % 80;
        }
      }
    });
  });
  rows.all(ComplexRows.delete, Runs.deletedCount, (d) {
    realm.write(() {
      for (var round = 0; round < Runs.deletedCount; round++) {
        final id = Runs.deleted(round);
        final found = realm.find<Person>(id);

        if (found == null) {
          continue;
        }

        final written = found.posts;
        final deleted = written.length;

        realm.deleteMany(written);
        realm.delete(found);
        d.add(id, deleted);
      }
    });
  });

  final leftPeople = Digest();
  final leftPosts = Digest();

  for (final p in realm.query<Person>('TRUEPREDICATE SORT(id ASC)')) {
    _person(leftPeople, p);
  }

  for (final p in realm.query<Post>('TRUEPREDICATE SORT(id ASC)')) {
    _post(leftPosts, p);
  }

  rows.check('left-people', leftPeople);
  rows.check('left-posts', leftPosts);
  realm.close();
}
