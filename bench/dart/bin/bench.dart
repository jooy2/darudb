/// The Dart side of the benchmark of `bench/README.md`: DaruDB and the
/// embedded stores Dart programs use most, each running the same workloads.
///
///   bench --stores --lock LOCK [--suite S]       the stores, with the versions LOCK resolved, as JSON
///   bench --child STORE --dir DIR [--suite S]    one pass of one store, one line of JSON per row
///
/// The suite is `simple`, by default, or `complex`, which only the stores
/// with a query language run.
///
/// `bench/run.mjs` runs the passes, each in a process of its own on new
/// files, and puts the runs together. It runs the harness compiled ahead of
/// time, with `dart build cli`, as a Flutter app runs.
library;

import 'dart:convert';
import 'dart:io';

import 'package:darudb_bench/common.dart';
import 'package:darudb_bench/complex_daru.dart' as complex_daru;
import 'package:darudb_bench/complex_sqlite.dart' as complex_sqlite;
import 'package:darudb_bench/daru.dart' as daru;
import 'package:darudb_bench/hive.dart' as hive;
import 'package:darudb_bench/sqlite.dart' as sqlite;
import 'package:sqlite3/sqlite3.dart' show sqlite3;

/// The version of [package] in the `pubspec.lock` at [lock].
String _locked(String lock, String package) {
  final match = RegExp(
    '\\n  $package:\\n(?:    .*\\n)*?    version: "([^"]+)"',
  ).firstMatch(File(lock).readAsStringSync());

  return match?.group(1) ?? '';
}

Future<void> main(List<String> args) async {
  final suite = args.contains('--suite')
      ? args[args.indexOf('--suite') + 1]
      : 'simple';

  if (suite != 'simple' && suite != 'complex') {
    stderr.writeln('no suite is named $suite');
    exit(2);
  }

  if (args.contains('--stores')) {
    final lock = args[args.indexOf('--lock') + 1];

    stdout.writeln(
      jsonEncode([
        {'id': 'daru', 'version': _locked(lock, 'darudb')},
        {
          'id': 'sqlite',
          'version':
              '${sqlite3.version.libVersion} (sqlite3 ${_locked(lock, 'sqlite3')})',
        },
        if (suite == 'simple')
          {'id': 'hive', 'version': 'hive_ce ${_locked(lock, 'hive_ce')}'},
      ]),
    );
    return;
  }

  final store = args[args.indexOf('--child') + 1];
  final directory = args[args.indexOf('--dir') + 1];
  final rows = Rows();

  Directory(directory).createSync(recursive: true);

  switch ((suite, store)) {
    case ('complex', 'daru'):
      complex_daru.run(directory, rows);
    case ('complex', 'sqlite'):
      complex_sqlite.run(directory, rows);
    case ('complex', _):
      stderr.writeln('no store is named $store in the complex suite');
      exit(2);
    case (_, 'daru'):
      daru.run(directory, rows);
    case (_, 'sqlite'):
      sqlite.run(directory, rows);
    case (_, 'hive'):
      await hive.run(directory, rows);
    default:
      stderr.writeln('no store is named $store');
      exit(2);
  }

  rows.finish();
}
