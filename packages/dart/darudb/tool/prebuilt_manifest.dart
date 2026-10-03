// Writes `hook/prebuilt.json` for a release, from a folder of the libraries
// the release workflow built: each named `darudb_dart-<target>.<extension>`,
// for every target `native/rust-toolchain.toml` lists.
//
//     dart --packages=.dart_tool/package_config.json \
//         tool/prebuilt_manifest.dart <folder> <base-url>
//
// Run that way rather than with `dart run`, it skips the package's build
// hook, which in a checkout would compile the engine first. `<base-url>` is
// where the release serves those files, ending in `/`. The manifest goes into
// the copy of the package that is published, never into the repository,
// whose manifest lists no library so that a checkout builds the engine from
// source.
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';

void main(List<String> args) {
  if (args.length != 2 || !args[1].endsWith('/')) {
    stderr.writeln(
      'usage: dart tool/prebuilt_manifest.dart <folder> <base-url>/',
    );
    exit(64);
  }

  final toolchain = File('native/rust-toolchain.toml').readAsStringSync();
  final listed = RegExp(
    r'^targets = \[(.*?)\]',
    multiLine: true,
    dotAll: true,
  ).firstMatch(toolchain)!.group(1)!;
  final targets = [
    for (final match in RegExp(r'"([^"]+)"').allMatches(listed))
      match.group(1)!,
  ];
  final files = {
    for (final entity in Directory(args[0]).listSync())
      if (entity is File) entity.uri.pathSegments.last: entity,
  };
  final libraries = <String, Object>{};

  for (final target in targets) {
    final name = files.keys.where(
      (name) => name.startsWith('darudb_dart-$target.'),
    );

    if (name.length != 1) {
      stderr.writeln('${args[0]} has no single library for $target');
      exit(1);
    }

    libraries[target] = {
      'file': name.single,
      'sha256': sha256
          .convert(files[name.single]!.readAsBytesSync())
          .toString(),
    };
  }

  File('hook/prebuilt.json').writeAsStringSync(
    '${const JsonEncoder.withIndent('  ').convert({'base': args[1], 'libraries': libraries})}\n',
  );
  stdout.writeln('hook/prebuilt.json lists ${libraries.length} libraries');
}
