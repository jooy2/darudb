/// Realm through its Dart SDK, for Flutter and Dart applications that move
/// from it: its vendor deprecated it in September 2024 and ended support in
/// September 2025. As in the Node.js harness, every commit syncs, since
/// Realm offers no commit that leaves the sync for later, so the deferred row
/// has no Realm cell. Realm assigns no keys, so the application gives each
/// object the key the other stores assign, and it has no unique index besides
/// the primary key, so the email is an index. Queries are Realm Query
/// Language strings, which Realm parses on every call, since it has no
/// prepared queries; sorting by `id` gives the order the other stores'
/// results come in. Every object read is copied out of Realm into the
/// fields the other stores give, as an application keeps them, and an update
/// sets the one property.
library;

import 'package:darudb_bench_pinned/model.dart';
import 'package:realm_dart/realm.dart';

// The shared workloads, from the harness beside this one, which this
// package cannot depend on, since it resolves DaruDB's code generator. A
// file under `bin/` reaches it by its path.
import '../../../dart/lib/common.dart';

Fields _copy(Person p) => Fields(p.id, p.name, p.email, p.age, p.city, p.score);

Person _keyed(int id, Fields p) =>
    Person(id, p.name, p.email, p.age, p.city, p.score);

void run(String directory, Rows rows) {
  Realm open(String name) =>
      Realm(Configuration.local([Person.schema], path: '$directory/$name'));

  var realm = open('commits.realm');

  rows.each('insert-sync', 500, (round, _) {
    realm.write(() => realm.add(_keyed(round + 1, person(round))));
  });
  rows.none('insert-deferred');
  realm.close();

  realm = open('objects.realm');

  rows.all('insert-bulk', objects, (_) {
    realm.write(() {
      for (var n = 0; n < objects; n++) {
        realm.add(_keyed(n + 1, person(n)));
      }
    });
  });

  final ids = randomIds(objects);
  final emails = [for (final n in randomNumbers(20000)) '$n@example.com'];

  void digest(Digest d, Person p) {
    final copy = _copy(p);

    d.add(copy.id, copy.age);
  }

  void each(Digest d, Iterable<Person> found) {
    for (final p in found) {
      digest(d, p);
    }
  }

  rows.each('get-key', objects, (round, d) {
    final found = realm.find<Person>(ids[round]);

    if (found != null) digest(d, found);
  });
  rows.each('get-email', 20000, (round, d) {
    final found = realm.query<Person>(r'email == $0', [emails[round]]);

    if (found.isNotEmpty) digest(d, found.first);
  });
  rows.each('age-equal', 200, (round, d) {
    each(d, realm.query<Person>(r'age == $0 SORT(id ASC)', [round % 80]));
  });
  rows.each('age-range', 5000, (round, d) {
    final age = round % 76;

    each(
      d,
      realm.query<Person>(
        r'age BETWEEN {$0, $1} SORT(age DESC, id ASC) LIMIT(20)',
        [age, age + 4],
      ),
    );
  });
  rows.each('count', 200, (_, d) {
    d.number(realm.query<Person>(r'age >= $0', [40]).length);
  });
  rows.each('city-scan', 10, (round, d) {
    each(
      d,
      realm.query<Person>(r'city == $0 SORT(id ASC)', ['city ${round % 100}']),
    );
  });
  rows.each('top-score', 10, (_, d) {
    each(
      d,
      realm.query<Person>('TRUEPREDICATE SORT(score DESC, id ASC) LIMIT(10)'),
    );
  });

  rows.all('update', 10000, (_) {
    realm.write(() {
      for (var round = 0; round < 10000; round++) {
        final id = ids[round];
        final found = realm.find<Person>(id);

        if (found != null) found.age = (id + 1) % 80;
      }
    });
  });
  rows.all('delete', 10000, (d) {
    realm.write(() {
      for (var round = 0; round < 10000; round++) {
        final found = realm.find<Person>(1 + round * 7);

        if (found != null) {
          realm.delete(found);
          d.number(1);
        }
      }
    });
  });

  final left = Digest();

  each(left, realm.query<Person>('TRUEPREDICATE SORT(id ASC)'));
  rows.check('left', left);
  realm.close();
}
