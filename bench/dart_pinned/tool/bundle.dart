/// Puts Realm's native library beside the harness `dart build cli` made,
/// where a compiled program looks for it: `dart run realm_dart install`
/// leaves it in the package's folder, which only a program run from inside
/// this package finds.
library;

import 'dart:convert';
import 'dart:io';

void main() {
  final config = File('.dart_tool/package_config.json');
  final packages = (jsonDecode(config.readAsStringSync()) as Map)['packages'];
  final root = (packages as List).cast<Map>().firstWhere(
        (package) => package['name'] == 'realm_dart',
      )['rootUri'] as String;
  final package =
      config.absolute.uri.resolve(root.endsWith('/') ? root : '$root/');
  final (os, name) = Platform.isMacOS
      ? ('macos', 'librealm_dart.dylib')
      : Platform.isWindows
          ? ('windows', 'realm_dart.dll')
          : ('linux', 'librealm_dart.so');
  final library = File.fromUri(package.resolve('binary/$os/$name'));

  if (!library.existsSync()) {
    stderr.writeln(
        '${library.path} is missing: run `dart run realm_dart install` first.');
    exit(1);
  }

  for (final target
      in Directory('build/cli').listSync().whereType<Directory>()) {
    library.copySync('${target.path}/bundle/bin/$name');
    stdout.writeln('copied $name into ${target.path}/bundle/bin');
  }
}
