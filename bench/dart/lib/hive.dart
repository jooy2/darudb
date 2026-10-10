/// Hive CE, which keeps every object of a box in memory and appends what is
/// written to the box's file. It has no transactions and no indexes: a sync
/// commit is a write followed by `flush`, which syncs the file; a deferred one
/// leaves out the flush, which the run makes once at the end; a transaction
/// is the writes of its objects followed by one flush. The unique email is a
/// second box from the email to the key, as an application would keep one,
/// and the queries on age read every object, as Hive's own `values` do.
library;

import 'package:hive_ce/hive.dart';

import 'common.dart';

final class _Adapter extends TypeAdapter<Fields> {
  @override
  final int typeId = 0;

  @override
  Fields read(BinaryReader reader) => Fields(
    reader.readInt(),
    reader.readString(),
    reader.readString(),
    reader.readInt(),
    reader.readString(),
    reader.readDouble(),
  );

  @override
  void write(BinaryWriter writer, Fields p) {
    writer
      ..writeInt(p.id)
      ..writeString(p.name)
      ..writeString(p.email)
      ..writeInt(p.age)
      ..writeString(p.city)
      ..writeDouble(p.score);
  }
}

Fields _keyed(int id, Fields p) =>
    Fields(id, p.name, p.email, p.age, p.city, p.score);

/// The ten highest scores among [people], ties going to the lower key.
List<Fields> _top(Iterable<Fields> people) {
  final top = <Fields>[];
  bool better(Fields a, Fields b) =>
      a.score > b.score || (a.score == b.score && a.id < b.id);

  for (final p in people) {
    if (top.length == 10 && !better(p, top[9])) continue;

    var at = top.indexWhere((kept) => better(p, kept));

    if (at < 0) at = top.length;
    top.insert(at, p);
    if (top.length > 10) top.removeLast();
  }

  return top;
}

Future<void> run(String directory, Rows rows) async {
  Hive
    ..init(directory)
    ..registerAdapter(_Adapter());

  var people = await Hive.openBox<Fields>('commits');
  var email = await Hive.openBox<int>('commits_email');
  var next = 1;

  Future<void> insert(Fields p) async {
    final id = next++;

    await people.put(id, _keyed(id, p));
    await email.put(p.email, id);
  }

  await rows.eachAsync('insert-sync', 500, (round, _) async {
    await insert(person(round));
    await people.flush();
    await email.flush();
  });
  await rows.eachAsync('insert-deferred', 10000, (round, _) async {
    await insert(person(1000 + round));
  });
  await people.flush();
  await email.flush();
  await Hive.close();

  people = await Hive.openBox<Fields>('objects');
  email = await Hive.openBox<int>('objects_email');

  await rows.allAsync('insert-bulk', objects, (_) async {
    await Future.wait([
      people.putAll({
        for (var n = 0; n < objects; n++) n + 1: _keyed(n + 1, person(n)),
      }),
      email.putAll({for (var n = 0; n < objects; n++) '$n@example.com': n + 1}),
    ]);
    await people.flush();
    await email.flush();
  });

  final ids = randomIds(objects);
  final emails = [for (final n in randomNumbers(20000)) '$n@example.com'];

  void digest(Digest d, Fields p) => d.add(p.id, p.age);

  rows.each('get-key', objects, (round, d) {
    final found = people.get(ids[round]);

    if (found != null) digest(d, found);
  });
  rows.each('get-email', 20000, (round, d) {
    final id = email.get(emails[round]);

    if (id != null) digest(d, people.get(id)!);
  });
  rows.each('age-equal', 200, (round, d) {
    final age = round % 80;

    for (final p in people.values) {
      if (p.age == age) digest(d, p);
    }
  });
  rows.each('age-range', 5000, (round, d) {
    final low = round % 76;
    final found = [
      for (final p in people.values)
        if (p.age >= low && p.age <= low + 4) p,
    ]..sort((a, b) => b.age != a.age ? b.age - a.age : a.id - b.id);

    for (final p in found.take(20)) {
      digest(d, p);
    }
  });
  rows.each('count', 200, (_, d) {
    d.number(people.values.where((p) => p.age >= 40).length);
  });
  rows.each('city-scan', 10, (round, d) {
    final city = 'city ${round % 100}';

    for (final p in people.values) {
      if (p.city == city) digest(d, p);
    }
  });
  rows.each('top-score', 10, (_, d) {
    for (final p in _top(people.values)) {
      digest(d, p);
    }
  });

  await rows.allAsync('update', 10000, (_) async {
    final writes = <Future<void>>[];

    for (var round = 0; round < 10000; round++) {
      final id = ids[round];
      final found = people.get(id);

      if (found != null) {
        writes.add(
          people.put(
            id,
            Fields(
              id,
              found.name,
              found.email,
              (id + 1) % 80,
              found.city,
              found.score,
            ),
          ),
        );
      }
    }

    await Future.wait(writes);
    await people.flush();
  });
  await rows.allAsync('delete', 10000, (d) async {
    final writes = <Future<void>>[];

    for (var round = 0; round < 10000; round++) {
      final id = 1 + round * 7;
      final found = people.get(id);

      if (found != null) {
        writes
          ..add(people.delete(id))
          ..add(email.delete(found.email));
        d.number(1);
      }
    }

    await Future.wait(writes);
    await people.flush();
    await email.flush();
  });

  final left = Digest();

  for (final p in people.values) {
    digest(left, p);
  }

  rows.check('left', left);
  await Hive.close();
}
