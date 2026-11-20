// The controller beside a screen that goes away: the screen disposes the
// controller and closes the store without waiting for a refresh that a tool
// started as it ended, so that refresh has to end without an error.
import 'dart:io';

import 'package:darudb/darudb.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:darudb_sample/src/store.dart';
import 'package:darudb_sample/src/ui/controller.dart';

void main() {
  late Directory directory;
  late SampleStore store;
  late SampleController controller;

  setUp(() async {
    directory = Directory.systemTemp.createTempSync('darudb_sample_controller');
    store = await SampleStore.open(directory.path);
    controller = SampleController(store);
  });

  tearDown(() {
    directory.deleteSync(recursive: true);
  });

  test('a refresh under way when the store closes ends quietly', () async {
    final Future<void> refresh = controller.refresh();

    controller.dispose();
    await store.close();
    await refresh;

    expect(controller.info, isNull);
  });

  test('a refresh begun after the store closed ends quietly', () async {
    controller.dispose();
    await store.close();
    await controller.refresh();

    expect(controller.info, isNull);
  });

  test('a refresh of a closed store fails while the screen is there', () async {
    await store.close();

    await expectLater(
      controller.refresh(),
      throwsA(
        isA<DaruException>().having(
          (DaruException error) => error.code,
          'code',
          'CLOSED',
        ),
      ),
    );

    controller.dispose();
  });
}
