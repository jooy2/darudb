// The sample from a user's side, in the app itself on a desktop: an empty
// database, sample data inserted in bulk, the objects filtered and paged by
// the engine, one object added, changed, refused and deleted, the file
// checked and compacted, the data still there when the app starts again, and
// the file reset. The steps run in order on one database folder, and each
// starts the app on it anew.
//
// Run with `flutter test integration_test -d macos` (or `-d linux`,
// `-d windows`).
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/ui/app.dart';
import 'package:darudb_sample/src/ui/home_page.dart';
import 'package:darudb_sample/src/ui/object_form.dart';

const Duration _timeout = Duration(seconds: 90);

/// Pumps frames until [finder] finds something, for work that waits on the
/// engine's threads rather than on the clock of the test.
Future<void> _pumpUntil(WidgetTester tester, Finder finder) async {
  final DateTime end = DateTime.now().add(_timeout);

  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 100));

    if (finder.evaluate().isNotEmpty) {
      return;
    }
  }

  throw TestFailure('timed out waiting for $finder');
}

Finder _textWithKey(Key key, bool Function(String text) test) =>
    find.byWidgetPredicate(
      (Widget widget) =>
          widget is Text &&
          widget.key == key &&
          widget.data != null &&
          test(widget.data!),
    );

Future<void> _waitForCount(
  WidgetTester tester,
  SampleCollection collection,
  String count,
) => _pumpUntil(
  tester,
  _textWithKey(countKey(collection), (String text) => text == count),
);

Future<void> _waitForTotal(
  WidgetTester tester,
  bool Function(String total) test,
) => _pumpUntil(tester, _textWithKey(const Key('total'), test));

Future<void> _tap(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.pump();
  await tester.tap(finder);
  await tester.pump();
}

Future<void> _type(WidgetTester tester, Finder field, String text) async {
  final Finder editable = find.descendant(
    of: field,
    matching: find.byType(EditableText),
  );

  await tester.ensureVisible(editable);
  await tester.tap(editable);
  await tester.pump();
  await tester.enterText(editable, text);
  await tester.testTextInput.receiveAction(TextInputAction.done);
  await tester.pump();
}

Future<void> _startApp(WidgetTester tester, String directory) async {
  await tester.pumpWidget(SampleApp(directory: directory));
  await _pumpUntil(tester, find.byKey(countKey(SampleCollection.people)));
}

Future<void> _applyFilter(WidgetTester tester, String filter) async {
  await _type(tester, find.byKey(const Key('filter')), filter);
  await _tap(tester, find.text('Apply'));
}

Future<void> _insertSampleData(WidgetTester tester) async {
  await _type(tester, find.byKey(const Key('seed')), '7');
  await _tap(tester, find.text('Insert sample data'));
  await _pumpUntil(tester, find.text('Inserted 4,020 objects'));
}

Future<void> _openCollection(
  WidgetTester tester,
  SampleCollection collection,
) async {
  await _tap(tester, find.text(collection.name).first);
  await _pumpUntil(tester, find.byKey(ValueKey<SampleCollection>(collection)));
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  late Directory directory;

  setUpAll(() {
    directory = Directory.systemTemp.createTempSync('darudb_sample_app');
  });

  tearDownAll(() {
    directory.deleteSync(recursive: true);
  });

  testWidgets('starts with an empty database', (WidgetTester tester) async {
    await _startApp(tester, directory.path);

    for (final SampleCollection collection in SampleCollection.values) {
      await _waitForCount(tester, collection, '0');
    }

    expect(find.text('No objects'), findsOneWidget);
  });

  testWidgets('inserts sample data in bulk, and continues its numbers', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path);
    await _insertSampleData(tester);
    await _waitForCount(tester, SampleCollection.organizations, '20');
    await _waitForCount(tester, SampleCollection.people, '1,000');
    await _waitForCount(tester, SampleCollection.posts, '3,000');
    expect(find.byKey(const Key('report')), findsOneWidget);

    // The same seed again would repeat every nickname and code, and the
    // unique index would refuse them, if a run did not continue from the
    // file's own numbers.
    await _insertSampleData(tester);
    await _waitForCount(tester, SampleCollection.organizations, '40');
    await _waitForCount(tester, SampleCollection.people, '2,000');
    await _waitForCount(tester, SampleCollection.posts, '6,000');
  });

  testWidgets('filters people and posts in the query language', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path);
    await _waitForTotal(tester, (String total) => total == '2,000');
    await _applyFilter(tester, 'age >= 65');
    await _waitForTotal(
      tester,
      (String total) => total != '2,000' && total != '0',
    );

    await _openCollection(tester, SampleCollection.posts);
    await _waitForTotal(tester, (String total) => total == '6,000');
    await _applyFilter(tester, 'author.language == "ko"');
    await _waitForTotal(
      tester,
      (String total) => total != '6,000' && total != '0',
    );

    await _applyFilter(tester, 'likes >>');
    await _pumpUntil(
      tester,
      find.descendant(
        of: find.byKey(const Key('list-failure')),
        matching: find.text('INVALID_QUERY'),
      ),
    );
  });

  testWidgets('adds, changes, refuses and deletes a person', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path);
    await _tap(tester, find.text('Add object'));
    await _pumpUntil(tester, find.byKey(const Key('save')));
    await _type(tester, find.byKey(fieldKey('name')), 'End To End');
    await _type(tester, find.byKey(fieldKey('nickname')), 'e2e-tester');
    await _type(tester, find.byKey(fieldKey('age')), '33');
    await _type(tester, find.byKey(fieldKey('gender')), 'female');
    await _type(tester, find.byKey(fieldKey('language')), 'en');
    await _type(tester, find.byKey(fieldKey('tags')), 'sample, test');
    await _tap(tester, find.byKey(const Key('save')));
    await _pumpUntil(tester, find.textContaining('Inserted people'));
    await _waitForCount(tester, SampleCollection.people, '2,001');

    await _applyFilter(tester, 'nickname == "e2e-tester"');
    await _waitForTotal(tester, (String total) => total == '1');
    await _tap(tester, find.text('Edit'));
    await _pumpUntil(tester, find.byKey(const Key('save')));
    await _type(tester, find.byKey(fieldKey('age')), '34');
    await _tap(tester, find.byKey(const Key('save')));
    await _pumpUntil(tester, find.textContaining('Updated people'));
    await _pumpUntil(tester, find.text('34'));

    // A second person with the same nickname: the unique index refuses it.
    await _tap(tester, find.text('Add object'));
    await _pumpUntil(tester, find.byKey(const Key('save')));
    await _type(tester, find.byKey(fieldKey('name')), 'Someone Else');
    await _type(tester, find.byKey(fieldKey('nickname')), 'e2e-tester');
    await _type(tester, find.byKey(fieldKey('gender')), 'male');
    await _type(tester, find.byKey(fieldKey('language')), 'en');
    await _tap(tester, find.byKey(const Key('save')));
    await _pumpUntil(
      tester,
      find.descendant(
        of: find.byKey(const Key('form-failure')),
        matching: find.text('DUPLICATE_KEY'),
      ),
    );
    await _tap(tester, find.text('Cancel'));
    await _waitForCount(tester, SampleCollection.people, '2,001');

    await _tap(tester, find.text('Delete'));
    await _pumpUntil(tester, find.textContaining('Delete people'));
    await _tap(tester, find.text('Delete').last);
    await _waitForTotal(tester, (String total) => total == '0');
    await _waitForCount(tester, SampleCollection.people, '2,000');
  });

  testWidgets('adds an organization under a key of its own', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path);
    await _openCollection(tester, SampleCollection.organizations);
    await _tap(tester, find.text('Add object'));
    await _pumpUntil(tester, find.byKey(const Key('save')));
    await _type(tester, find.byKey(fieldKey('code')), 'E2E-0001');
    await _type(tester, find.byKey(fieldKey('name')), 'End To End Ltd.');
    await _type(tester, find.byKey(fieldKey('kind')), 'company');
    await _type(tester, find.byKey(fieldKey('language')), 'en');
    await _type(tester, find.byKey(fieldKey('founded')), '2024');
    await _tap(tester, find.byKey(const Key('save')));
    await _pumpUntil(tester, find.text('Inserted organizations E2E-0001'));
    await _waitForCount(tester, SampleCollection.organizations, '41');
  });

  testWidgets('checks and compacts the file', (WidgetTester tester) async {
    await _startApp(tester, directory.path);
    await _tap(tester, find.text('Check'));
    await _pumpUntil(tester, find.text('The file is intact'));
    await _tap(tester, find.text('Compact'));
    await _pumpUntil(tester, find.text('Compacted'));
  });

  testWidgets('keeps what it committed when the app starts again', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path);
    await _waitForCount(tester, SampleCollection.organizations, '41');
    await _waitForCount(tester, SampleCollection.people, '2,000');
    await _waitForCount(tester, SampleCollection.posts, '6,000');
  });

  testWidgets('resets the database', (WidgetTester tester) async {
    await _startApp(tester, directory.path);
    await _tap(tester, find.text('Reset'));
    await _pumpUntil(tester, find.text('Delete every object?'));
    await _tap(tester, find.text('Reset').last);
    await _pumpUntil(tester, find.text('The database is empty'));

    for (final SampleCollection collection in SampleCollection.values) {
      await _waitForCount(tester, collection, '0');
    }
  });
}
