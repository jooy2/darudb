// The prebuilt native libraries of a release, which the build hook downloads
// rather than compiling the engine, so that an application's developers need
// no Rust toolchain.
//
// `hook/prebuilt.json` names, for each Rust target, the file of the release
// that holds its library and the SHA-256 hash that file has. It ships inside
// the package, so the package itself vouches for what is downloaded: a file
// with any other hash is refused, wherever it came from. In this repository
// the manifest lists no library, and the hook builds the engine from source;
// the release workflow fills it in the copy it publishes.
import 'dart:convert';
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';

/// A failure of the hook to provide the native library, with what to do
/// about it.
final class PrebuiltException implements Exception {
  const PrebuiltException(this.message);

  final String message;

  @override
  String toString() => 'darudb: $message';
}

/// One target's library in a release.
final class PrebuiltLibrary {
  const PrebuiltLibrary(this.file, this.sha256);

  /// The file's name in the release.
  final String file;

  /// The SHA-256 hash of the file, in lowercase hexadecimal.
  final String sha256;
}

/// The libraries of one release, as `hook/prebuilt.json` lists them.
final class Prebuilt {
  const Prebuilt(this.base, this.libraries);

  /// Reads the manifest, and checks it: a manifest that does not fit could
  /// only send the hook somewhere it should not go.
  factory Prebuilt.parse(String text) {
    final Object? json;

    try {
      json = jsonDecode(text);
    } on FormatException catch (error) {
      throw PrebuiltException(
        'hook/prebuilt.json is not JSON: ${error.message}',
      );
    }

    if (json is! Map<String, Object?> ||
        json['libraries'] is! Map<String, Object?>) {
      throw const PrebuiltException(
        'hook/prebuilt.json has no `libraries` object',
      );
    }

    final libraries = <String, PrebuiltLibrary>{};

    for (final MapEntry(key: target, :value)
        in (json['libraries'] as Map<String, Object?>).entries) {
      if (value case {
        'file': final String file,
        'sha256': final String hash,
      } when _fileName.hasMatch(file) && _hash.hasMatch(hash)) {
        libraries[target] = PrebuiltLibrary(file, hash);
      } else {
        throw PrebuiltException(
          'hook/prebuilt.json lists `$target` without a plain file name and '
          'a SHA-256 hash in lowercase hexadecimal',
        );
      }
    }

    final base = switch (json['base']) {
      null when libraries.isEmpty => null,
      final String text => Uri.tryParse(text),
      _ => null,
    };

    if (libraries.isNotEmpty && (base == null || !_trusted(base))) {
      throw const PrebuiltException(
        'hook/prebuilt.json lists libraries without an https `base` that ends '
        'in `/`',
      );
    }

    return Prebuilt(base, libraries);
  }

  /// Where the release's files are, or `null` when there are none.
  final Uri? base;

  /// Each library, by the Rust target it was built for.
  final Map<String, PrebuiltLibrary> libraries;

  /// Whether the manifest lists no library, as in this repository, where
  /// the hook builds the engine from source.
  bool get isEmpty => libraries.isEmpty;

  static final _fileName = RegExp(r'^[A-Za-z0-9._-]+$');
  static final _hash = RegExp(r'^[0-9a-f]{64}$');

  /// An https address, or a plain one on the loopback interface, for tests.
  static bool _trusted(Uri base) =>
      base.path.endsWith('/') &&
      (base.isScheme('https') ||
          (base.isScheme('http') &&
              (base.host == '127.0.0.1' || base.host == 'localhost')));
}

/// The Rust target that builds the engine for [code], as the triples the
/// manifest and the crate's `rust-toolchain.toml` name them.
String targetOf(CodeConfig code) =>
    switch ((code.targetOS, code.targetArchitecture)) {
      (OS.android, Architecture.arm64) => 'aarch64-linux-android',
      (OS.android, Architecture.arm) => 'armv7-linux-androideabi',
      (OS.android, Architecture.x64) => 'x86_64-linux-android',
      (OS.iOS, Architecture.arm64) =>
        code.iOS.targetSdk == IOSSdk.iPhoneSimulator
            ? 'aarch64-apple-ios-sim'
            : 'aarch64-apple-ios',
      (OS.iOS, Architecture.x64) => 'x86_64-apple-ios',
      (OS.macOS, Architecture.arm64) => 'aarch64-apple-darwin',
      (OS.macOS, Architecture.x64) => 'x86_64-apple-darwin',
      (OS.windows, Architecture.arm64) => 'aarch64-pc-windows-msvc',
      (OS.windows, Architecture.x64) => 'x86_64-pc-windows-msvc',
      (OS.linux, Architecture.arm64) => 'aarch64-unknown-linux-gnu',
      (OS.linux, Architecture.x64) => 'x86_64-unknown-linux-gnu',
      (final os, final architecture) => throw PrebuiltException(
        'DaruDB does not build for $os on $architecture',
      ),
    };

/// Makes [into] hold [library] of [prebuilt], and returns it. A file already
/// there with the library's hash is kept, so a library is downloaded once
/// for every build that shares the hook's cache. A download is written beside
/// [into] and moved into place only once its hash matches.
Future<File> fetch(
  Prebuilt prebuilt,
  PrebuiltLibrary library,
  File into, {
  int attempts = 3,
}) async {
  if (await into.exists() && await _hashOf(into) == library.sha256) {
    return into;
  }

  final url = prebuilt.base!.resolve(library.file);
  final part = File('${into.path}.part');

  await into.parent.create(recursive: true);

  for (var attempt = 1; ; attempt++) {
    try {
      final hash = await _download(url, part);

      if (hash != library.sha256) {
        // The file is not the one the package names. Downloading it again
        // would not change that, so the build stops here.
        await part.delete();

        throw PrebuiltException(
          '$url has the SHA-256 hash $hash, not the ${library.sha256} '
          'hook/prebuilt.json names, so it was not used',
        );
      }

      if (await into.exists()) {
        // A file with another hash, which Windows would not replace.
        await into.delete();
      }

      return await part.rename(into.path);
    } on IOException catch (error) {
      if (attempt >= attempts) {
        throw PrebuiltException(
          'could not download $url: $error. A build without network access '
          'needs the library in the build cache already, or a dependency on '
          'the package from a checkout of its repository, which builds the '
          'engine from source',
        );
      }

      await Future<void>.delayed(Duration(seconds: attempt));
    }
  }
}

/// Writes what [url] serves into [file], and returns its SHA-256 hash.
Future<String> _download(Uri url, File file) async {
  final client = HttpClient()
    ..connectionTimeout = const Duration(seconds: 30)
    ..findProxy = HttpClient.findProxyFromEnvironment;

  try {
    final request = await client.getUrl(url);
    final response = await request.close();

    if (response.statusCode != HttpStatus.ok) {
      await response.drain<void>();

      throw HttpException('status ${response.statusCode}', uri: url);
    }

    final digest = _Digest();
    final sink = sha256.startChunkedConversion(digest);
    final out = file.openWrite();

    try {
      await for (final chunk in response) {
        sink.add(chunk);
        out.add(chunk);
      }
    } finally {
      await out.close();
    }

    sink.close();

    return digest.value.toString();
  } finally {
    client.close(force: true);
  }
}

Future<String> _hashOf(File file) async =>
    (await sha256.bind(file.openRead()).first).toString();

/// Receives the one digest a chunked hash produces.
final class _Digest implements Sink<Digest> {
  late Digest value;

  @override
  void add(Digest data) => value = data;

  @override
  void close() {}
}
