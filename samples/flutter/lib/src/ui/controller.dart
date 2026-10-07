// What the screen remembers between frames: the file's counts, the chosen
// collection, whether a tool or a sample run is going on, and how far it has
// come. Every change to the file bumps `version`, which the list watches to
// read its page again.
import 'dart:async';

import 'package:flutter/foundation.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/store.dart';

final class SampleController extends ChangeNotifier {
  SampleController(this.store);

  final SampleStore store;
  SampleInfo? info;
  SampleCollection collection = SampleCollection.people;
  int version = 0;
  bool busy = false;
  SeedProgress? progress;
  SeedReport? report;
  bool _disposed = false;

  void _notify() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  Future<void> refresh() async {
    final SampleInfo next = await store.info();

    info = next;
    _notify();
  }

  void select(SampleCollection next) {
    collection = next;
    _notify();
  }

  /// Says the file changed: the counts and the list are read again.
  void changed() {
    version += 1;
    _notify();
    unawaited(refresh());
  }

  /// Runs [work] with the tools held off until it ends.
  Future<T> runTool<T>(Future<T> Function() work) async {
    busy = true;
    _notify();

    try {
      return await work();
    } finally {
      busy = false;
      changed();
    }
  }

  Future<SeedReport> seed({required int people, required int seed}) =>
      runTool(() async {
        report = null;

        try {
          final SeedReport finished = await store.seed(
            people: people,
            seed: seed,
            onProgress: (SeedProgress next) {
              progress = next;
              _notify();
            },
          );

          report = finished;

          return finished;
        } finally {
          progress = null;
        }
      });

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}
