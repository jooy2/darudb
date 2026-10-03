// The build hook's prebuilt libraries: the manifest it trusts, and a download
// that is used only when its hash is the one the manifest names.
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:test/test.dart';

import '../hook/prebuilt.dart';

void main() {
  // The release workflow runs the tests with a manifest that lists the
  // library it built, and leaves this one out.
  test('the manifest in the repository lists no library', () {
    final prebuilt = Prebuilt.parse(
      File('hook/prebuilt.json').readAsStringSync(),
    );

    expect(prebuilt.isEmpty, isTrue);
    expect(prebuilt.base, isNull);
  }, tags: 'checkout');

  group('a manifest is refused', () {
    final hash = '0' * 64;

    for (final (name, text) in [
      ('when it is not JSON', '{'),
      ('without libraries', '{"base": null}'),
      (
        'with libraries and no base',
        '{"libraries": {"x": {"file": "a.so", "sha256": "$hash"}}}',
      ),
      (
        'with a plain base off the loopback interface',
        '{"base": "http://example.com/", '
            '"libraries": {"x": {"file": "a.so", "sha256": "$hash"}}}',
      ),
      (
        'with a base that does not end in a slash',
        '{"base": "https://example.com/v1", '
            '"libraries": {"x": {"file": "a.so", "sha256": "$hash"}}}',
      ),
      (
        'with a file name that is a path',
        '{"base": "https://example.com/", '
            '"libraries": {"x": {"file": "../a.so", "sha256": "$hash"}}}',
      ),
      (
        'with a hash that is not SHA-256',
        '{"base": "https://example.com/", '
            '"libraries": {"x": {"file": "a.so", "sha256": "abc"}}}',
      ),
    ]) {
      test(name, () {
        expect(() => Prebuilt.parse(text), throwsA(isA<PrebuiltException>()));
      });
    }
  });

  group('fetch', () {
    late HttpServer server;
    late Directory directory;
    late Map<String, List<int>> served;
    var requests = 0;

    setUp(() async {
      directory = Directory.systemTemp.createTempSync('darudb-prebuilt-');
      served = {};
      requests = 0;
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) {
        requests++;

        final body = served[request.uri.pathSegments.last];

        request.response.statusCode = body == null
            ? HttpStatus.notFound
            : HttpStatus.ok;

        if (body != null) {
          request.response.add(body);
        }

        request.response.close();
      });
    });

    tearDown(() async {
      await server.close(force: true);
      directory.deleteSync(recursive: true);
    });

    Prebuilt manifest(String file, String hash) => Prebuilt.parse(
      jsonEncode({
        'base': 'http://127.0.0.1:${server.port}/release/',
        'libraries': {
          'x86_64-unknown-linux-gnu': {'file': file, 'sha256': hash},
        },
      }),
    );

    test('downloads a library once, and keeps it', () async {
      final bytes = utf8.encode('a native library');

      served['lib.so'] = bytes;

      final prebuilt = manifest('lib.so', sha256.convert(bytes).toString());
      final library = prebuilt.libraries.values.single;
      final into = File('${directory.path}/cache/libdarudb_dart.so');

      expect((await fetch(prebuilt, library, into)).readAsBytesSync(), bytes);
      expect(requests, 1);

      expect((await fetch(prebuilt, library, into)).readAsBytesSync(), bytes);
      expect(requests, 1);
    });

    test('downloads again over a file with another hash', () async {
      final bytes = utf8.encode('the right library');

      served['lib.so'] = bytes;

      final prebuilt = manifest('lib.so', sha256.convert(bytes).toString());
      final into = File('${directory.path}/libdarudb_dart.so')
        ..writeAsStringSync('an old library');

      await fetch(prebuilt, prebuilt.libraries.values.single, into);

      expect(into.readAsBytesSync(), bytes);
      expect(requests, 1);
    });

    test('refuses a library with another hash, and keeps nothing', () async {
      served['lib.so'] = utf8.encode('not the library the manifest names');

      final prebuilt = manifest(
        'lib.so',
        sha256.convert(utf8.encode('the library it names')).toString(),
      );
      final into = File('${directory.path}/libdarudb_dart.so');

      await expectLater(
        fetch(prebuilt, prebuilt.libraries.values.single, into),
        throwsA(
          isA<PrebuiltException>().having(
            (error) => error.message,
            'message',
            contains('was not used'),
          ),
        ),
      );
      expect(directory.listSync(), isEmpty);
      // A file that does not match is not downloaded again.
      expect(requests, 1);
    });

    test('says what to do when the download fails', () async {
      final prebuilt = manifest('missing.so', '0' * 64);

      await expectLater(
        fetch(
          prebuilt,
          prebuilt.libraries.values.single,
          File('${directory.path}/libdarudb_dart.so'),
          attempts: 2,
        ),
        throwsA(
          isA<PrebuiltException>().having(
            (error) => error.message,
            'message',
            allOf(contains('status 404'), contains('checkout')),
          ),
        ),
      );
      expect(requests, 2);
    });
  });
}
