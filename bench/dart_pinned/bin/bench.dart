/// The Dart side of the benchmark of `bench/README.md` for Realm, whose code
/// generator cannot resolve beside DaruDB's: the same commands as the
/// harness of `bench/dart`, which `bench/run.mjs` runs beside it.
///
///   bench --stores --lock LOCK [--suite S]       the store, with the version LOCK resolved, as JSON
///   bench --child STORE --dir DIR [--suite S]    one pass of the store, one line of JSON per row
library;

import 'dart:convert';
import 'dart:io';

import '../../dart/lib/common.dart';
import 'src/complex_realm.dart' as complex_realm;
import 'src/realm.dart' as realm;

/// The version of [package] in the `pubspec.lock` at [lock].
String _locked(String lock, String package) {
  final match = RegExp(
    '\\n  $package:\\n(?:    .*\\n)*?    version: "([^"]+)"',
  ).firstMatch(File(lock).readAsStringSync());

  return match?.group(1) ?? '';
}

void main(List<String> args) {
  final suite =
      args.contains('--suite') ? args[args.indexOf('--suite') + 1] : 'simple';

  if (suite != 'simple' && suite != 'complex') {
    stderr.writeln('no suite is named $suite');
    exit(2);
  }

  if (args.contains('--stores')) {
    final lock = args[args.indexOf('--lock') + 1];

    stdout.writeln(
      jsonEncode([
        {'id': 'realm', 'version': 'realm_dart ${_locked(lock, 'realm_dart')}'},
      ]),
    );
    return;
  }

  final store = args[args.indexOf('--child') + 1];
  final directory = args[args.indexOf('--dir') + 1];
  final rows = Rows();

  Directory(directory).createSync(recursive: true);

  switch ((suite, store)) {
    case ('complex', 'realm'):
      complex_realm.run(directory, rows);
    case (_, 'realm'):
      realm.run(directory, rows);
    default:
      stderr.writeln('no store $store');
      exit(2);
  }

  rows.finish();
  // Realm keeps threads of its own that would hold the process open.
  exit(0);
}
