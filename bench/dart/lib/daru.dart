/// DaruDB through its Dart package, as an application uses it: a class the
/// generator reads, and queries that run many times prepared once.
library;

import 'package:darudb/darudb.dart';

import 'common.dart';
import 'model.dart';

const _schema = Schema(1, [personSchema]);

Person _person(int n) {
  final p = person(n);

  return Person(
    name: p.name,
    email: p.email,
    age: p.age,
    city: p.city,
    score: p.score,
  );
}

void _digest(Digest d, Person p) => d.add(p.id!, p.age);

void run(String directory, Rows rows) {
  var db = Database.open('$directory/commits.darudb', schema: _schema);

  rows.each('insert-sync', 500, (round, _) {
    db.write((txn) => txn.collection(personSchema).insert(_person(round)));
  });
  rows.each('insert-deferred', 10000, (round, _) {
    db.write(
      (txn) => txn.collection(personSchema).insert(_person(1000 + round)),
      durability: Durability.deferred,
    );
  });
  db.close();

  db = Database.open('$directory/objects.darudb', schema: _schema);

  rows.all('insert-bulk', objects, (_) {
    db.write((txn) {
      final people = txn.collection(personSchema);

      for (var n = 0; n < objects; n++) {
        people.insert(_person(n));
      }
    });
  });

  final ids = randomIds(objects);
  final emails = [for (final n in randomNumbers(20000)) '$n@example.com'];
  final byEmail = db.prepare(personSchema, r'email == $0');
  final byAge = db.prepare(personSchema, r'age == $0');
  final range = db.prepare(
    personSchema,
    r'age BETWEEN $0 AND $1 SORT BY age DESC LIMIT 20',
  );
  final atLeast = db.prepare(personSchema, r'age >= $0');
  final inCity = db.prepare(personSchema, r'city == $0');
  final top = db.prepare(personSchema, 'SORT BY score DESC LIMIT 10');

  db.read((txn) {
    final people = txn.collection(personSchema);

    rows.each('get-key', objects, (round, d) {
      final found = people.get(ids[round]);

      if (found != null) _digest(d, found);
    });
    rows.each('get-email', 20000, (round, d) {
      final found = people.findOnePrepared(byEmail, [emails[round]]);

      if (found != null) _digest(d, found);
    });
    rows.each('age-equal', 200, (round, d) {
      for (final found in people.findPrepared(byAge, [round % 80])) {
        _digest(d, found);
      }
    });
    rows.each('age-range', 5000, (round, d) {
      final age = round % 76;

      for (final found in people.findPrepared(range, [age, age + 4])) {
        _digest(d, found);
      }
    });
    rows.each('count', 200, (_, d) {
      d.number(people.countPrepared(atLeast, [40]));
    });
    rows.each('city-scan', 10, (round, d) {
      for (final found in people.findPrepared(inCity, [
        'city ${round % 100}',
      ])) {
        _digest(d, found);
      }
    });
    rows.each('top-score', 10, (_, d) {
      for (final found in people.findPrepared(top)) {
        _digest(d, found);
      }
    });
  });

  rows.all('update', 10000, (_) {
    db.write((txn) {
      final people = txn.collection(personSchema);

      for (var round = 0; round < 10000; round++) {
        final id = ids[round];
        final found = people.get(id);

        if (found != null) {
          people.put(found.copyWith(age: (id + 1) % 80));
        }
      }
    });
  });
  rows.all('delete', 10000, (d) {
    db.write((txn) {
      final people = txn.collection(personSchema);

      for (var round = 0; round < 10000; round++) {
        if (people.delete(1 + round * 7)) d.number(1);
      }
    });
  });

  final left = Digest();

  db.read((txn) {
    for (final found in txn.collection(personSchema).find()) {
      _digest(left, found);
    }
  });
  rows.check('left', left);
  db.close();
}
