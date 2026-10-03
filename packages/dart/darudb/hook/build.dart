// Builds the engine for the target the application is built for, from the
// Rust crate in `native/`, and gives the library the asset id
// `package:darudb/src/native.dart`, which `lib/src/native.dart` names.
import 'package:hooks/hooks.dart';
import 'package:native_toolchain_rust/native_toolchain_rust.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    await const RustBuilder(
      assetName: 'src/native.dart',
      cratePath: 'native',
    ).run(input: input, output: output);
  });
}
