// Seeded randomness for the sample data, so that one seed always makes the
// same objects. `randino` takes a `Random` where it would otherwise make one
// of its own, and `SeededRandom` is one; `Draw` adds the few helpers the
// generator needs.
//
// The generator is mulberry32, a small 32-bit generator in the public
// domain: fast, and good enough for sample data, though not for anything that
// needs to be unpredictable. `seedOf` mixes several numbers into one seed
// with the finalizer of MurmurHash3, so that neighbouring record numbers
// start from unrelated states. They are the functions of
// `samples/node/core/random.ts`, written for Dart's 64-bit integers.
import 'dart:math';

const int _mask32 = 0xffffffff;

int _multiply32(int a, int b) => (a * b) & _mask32;

int _finalize(int value) {
  int h = value & _mask32;

  h ^= h >> 16;
  h = _multiply32(h, 0x85ebca6b);
  h ^= h >> 13;
  h = _multiply32(h, 0xc2b2ae35);
  h ^= h >> 16;

  return h;
}

/// One 32-bit seed from several numbers, each taken as an unsigned 32-bit
/// integer.
int seedOf(List<int> parts) {
  int h = 0x9e3779b9;

  for (final int part in parts) {
    h = _finalize(((h ^ (part & _mask32)) + 0x7f4a7c15) & _mask32);
  }

  return h;
}

/// A `Random` that mulberry32 draws for, from [seed].
final class SeededRandom implements Random {
  SeededRandom(int seed) : _state = seed & _mask32;

  int _state;

  int _next() {
    _state = (_state + 0x6d2b79f5) & _mask32;

    int t = _multiply32(_state ^ (_state >> 15), 1 | _state);

    t = ((t + _multiply32(t ^ (t >> 7), 61 | t)) & _mask32) ^ t;

    return (t ^ (t >> 14)) & _mask32;
  }

  @override
  double nextDouble() => _next() / 4294967296;

  @override
  int nextInt(int max) {
    if (max <= 0 || max > 1 << 32) {
      throw RangeError.range(max, 1, 1 << 32, 'max');
    }

    return (nextDouble() * max).floor();
  }

  @override
  bool nextBool() => nextDouble() < 0.5;
}

/// Draws from one seed: numbers, picks and coin flips.
final class Draw {
  Draw(int seed) : random = SeededRandom(seed);

  final SeededRandom random;

  /// An integer from 0 up to, but not including, [bound], which may be past
  /// the 2^32 `Random.nextInt` takes.
  int below(int bound) => (random.nextDouble() * bound).floor();

  /// An integer from [low] to [high], both included.
  int between(int low, int high) => low + below(high - low + 1);

  /// Whether an event of probability [p] happens.
  bool chance(double p) => random.nextDouble() < p;

  T pick<T>(List<T> values) => values[below(values.length)];

  /// [count] different values, or all of them when there are fewer.
  List<T> picks<T>(List<T> values, int count) {
    final Set<int> chosen = <int>{};
    final int wanted = min(count, values.length);

    while (chosen.length < wanted) {
      chosen.add(below(values.length));
    }

    return <T>[for (final int index in chosen) values[index]];
  }

  /// A value of [values], each as likely as its weight.
  T weighted<T>(List<T> values, List<int> weights) {
    int total = 0;

    for (final int weight in weights) {
      total += weight;
    }

    double rest = random.nextDouble() * total;

    for (int index = 0; index < values.length; index += 1) {
      rest -= weights[index];

      if (rest < 0) {
        return values[index];
      }
    }

    return values.last;
  }
}
