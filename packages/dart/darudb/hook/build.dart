// Gives the package its native library, with the asset id
// `package:darudb/src/native.dart` that `lib/src/native.dart` names.
//
// A published copy of the package downloads the library built for the
// application's target from the package's GitHub release, and uses it only
// if its SHA-256 hash is the one `hook/prebuilt.json` names, as `prebuilt.dart`
// explains. A checkout of the repository, which an application gets by
// depending on the package through git or a path, lists no library there,
// and builds the engine from the Rust crate in `native/` for the target
// instead, with the toolchain `native/rust-toolchain.toml` pins.
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';
import 'package:native_toolchain_rust/native_toolchain_rust.dart';

import 'prebuilt.dart';

const _asset = 'src/native.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) {
      return;
    }

    final manifest = input.packageRoot.resolve('hook/prebuilt.json');

    output.dependencies.add(manifest);

    final prebuilt = Prebuilt.parse(
      await File.fromUri(manifest).readAsString(),
    );

    if (prebuilt.isEmpty) {
      await const RustBuilder(
        assetName: _asset,
        cratePath: 'native',
      ).run(input: input, output: output);

      return;
    }

    final code = input.config.code;
    final target = targetOf(code);

    if (code.linkModePreference == LinkModePreference.static) {
      throw const PrebuiltException(
        'the build asks for a static library, and the prebuilt ones are '
        'dynamic. Depend on the package from a checkout of its repository, '
        'which builds the engine from source',
      );
    }

    final library =
        prebuilt.libraries[target] ??
        (throw PrebuiltException(
          'no prebuilt library for $target. Depend on the package from a '
          'checkout of its repository, which builds the engine from source',
        ));
    final linkMode = DynamicLoadingBundled();
    final file = await fetch(
      prebuilt,
      library,
      File.fromUri(
        input.outputDirectoryShared.resolve(
          '$target/${library.sha256.substring(0, 16)}/'
          '${code.targetOS.libraryFileName('darudb_dart', linkMode)}',
        ),
      ),
    );

    // A library deleted from the cache, or changed there, runs the hook again.
    output.dependencies.add(file.uri);
    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: _asset,
        linkMode: linkMode,
        file: file.uri,
      ),
    );
  });
}
