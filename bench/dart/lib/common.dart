/// What every store runs: the objects, the order random reads go in, timing,
/// and the digest a row's results are checked with. `bench/README.md`
/// describes the workloads, and the other languages' harnesses draw the same
/// objects in the same order.
library;

import 'dart:convert';
import 'dart:io';

const int objects = 100000;

/// An object's fields, as the stores that keep no class of their own write
/// them and give them back.
final class Fields {
  Fields(this.id, this.name, this.email, this.age, this.city, this.score);

  final int id;
  final String name;
  final String email;
  int age;
  final String city;
  final double score;
}

/// The `n`th object, counting from 0, before it has a key. It gets `n + 1`.
Fields person(int n) => Fields(
  0,
  'person $n',
  '$n@example.com',
  n * 7919 % 80,
  'city ${n % 100}',
  (n * 0.618) % 1,
);

final BigInt _spread = BigInt.parse('9E3779B97F4A7C15', radix: 16);
final BigInt _mask = (BigInt.one << 64) - BigInt.one;

BigInt _scatter(int round) {
  final x = (BigInt.from(round) * _spread) & _mask;

  return ((x << 17) | (x >> 47)) & _mask;
}

/// The keys the random reads ask for, drawn before the timing starts.
List<int> randomIds(int count) => [
  for (var round = 0; round < count; round++)
    1 + (_scatter(round) % BigInt.from(objects)).toInt(),
];

/// The numbers of the objects the random email lookups ask for.
List<int> randomNumbers(int count) => [
  for (var round = 0; round < count; round++)
    (_scatter(round) % BigInt.from(objects)).toInt(),
];

/// How many results a row saw, and a 32-bit hash of their keys and ages.
final class Digest {
  int count = 0;
  int hash = 0;

  void add(int id, int age) {
    count++;
    hash = ((hash ^ ((id * 131 + age) & 0xFFFFFFFF)) * 0x01000193) & 0xFFFFFFFF;
  }

  void number(int n) => add(n, 0);
}

final class Rows {
  final List<Map<String, Object>> rows = [];

  /// Runs [step] [count] times and records the time each took on average.
  void each(String row, int count, void Function(int round, Digest d) step) {
    final digest = Digest();
    final watch = Stopwatch()..start();

    for (var round = 0; round < count; round++) {
      step(round, digest);
    }

    _push(row, watch.elapsedMicroseconds * 1000 / count, digest);
  }

  /// Runs [work] once, which does [count] operations and commits them.
  void all(String row, int count, void Function(Digest d) work) {
    final digest = Digest();
    final watch = Stopwatch()..start();

    work(digest);
    _push(row, watch.elapsedMicroseconds * 1000 / count, digest);
  }

  /// Runs [work] once, as [all] does, for a store whose writes are futures.
  Future<void> allAsync(
    String row,
    int count,
    Future<void> Function(Digest d) work,
  ) async {
    final digest = Digest();
    final watch = Stopwatch()..start();

    await work(digest);
    _push(row, watch.elapsedMicroseconds * 1000 / count, digest);
  }

  /// Runs [step] [count] times, as [each] does, for a store whose writes are
  /// futures.
  Future<void> eachAsync(
    String row,
    int count,
    Future<void> Function(int round, Digest d) step,
  ) async {
    final digest = Digest();
    final watch = Stopwatch()..start();

    for (var round = 0; round < count; round++) {
      await step(round, digest);
    }

    _push(row, watch.elapsedMicroseconds * 1000 / count, digest);
  }

  /// A row that is checked and not timed.
  void check(String row, Digest digest) => _push(row, 0, digest);

  void _push(String row, num ns, Digest digest) => rows.add({
    'row': row,
    'ns': ns,
    'count': digest.count,
    'hash': digest.hash,
  });

  /// Writes the rows for the parent, one line of JSON each.
  void finish() =>
      stdout.write(rows.map((row) => '${jsonEncode(row)}\n').join());
}
