// The sample from a user's side, in the app itself: an empty database, plain
// or made encrypted, sample data inserted in bulk, the objects filtered and
// paged by the engine, one object added, changed, refused and deleted, the
// file checked and compacted, the data still there when the app starts again,
// and the file reset. The steps run in order on one database folder, and each
// starts the app on it anew; the whole scenario runs on a plain file and
// again on an encrypted one.
//
// Run with `flutter test integration_test -d flutter-tester`, which lays the
// app out at its window's size with no window on the screen, or on a desktop
// with `-d macos`, `-d linux` or `-d windows`, which opens the app's window.
// Both run the plain file once more on a phone's screen, where the sidebar is
// a drawer. On a phone or its simulator, `-d <device>`, the scenario runs on
// the device's own screen.
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/ui/app.dart';
import 'package:darudb_sample/src/ui/home_page.dart';
import 'package:darudb_sample/src/ui/object_form.dart';

const Duration _timeout = Duration(seconds: 90);

/// A screen the scenario lays the app out on: its size in logical pixels, and
/// how many device pixels each one is.
typedef _Screen = ({Size size, double ratio});

/// The desktop app's window.
const _Screen _window = (size: Size(1360, 860), ratio: 1);

/// A phone held upright.
const _Screen _phone = (size: Size(390, 844), ratio: 3);

/// Whether the test runs on a phone or its simulator, whose own screen it
/// keeps.
final bool _onDevice = Platform.isAndroid || Platform.isIOS;

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

/// Whether the sidebar is a drawer, which the header's button opens: the
/// button draws itself only then.
bool _collapsed(WidgetTester tester) => find
    .descendant(of: find.byKey(menuKey), matching: find.byType(PlIconButton))
    .evaluate()
    .isNotEmpty;

/// The drawer's close button, there while the drawer is open or on its way.
Finder get _closeSidebar => find.bySemanticsLabel(closeSidebarLabel);

/// Pumps frames while [moving] holds, for an animation that has to end
/// before the next step reads the screen.
Future<void> _pumpWhile(
  WidgetTester tester,
  String what,
  bool Function() moving,
) async {
  final DateTime end = DateTime.now().add(_timeout);

  while (moving()) {
    if (DateTime.now().isAfter(end)) {
      throw TestFailure('timed out waiting for $what');
    }

    await tester.pump(const Duration(milliseconds: 100));
  }
}

/// Whether the drawer is open, once it has finished opening or closing: its
/// close button is in the tree from the moment it starts to open until it
/// has closed, and can be pressed only while it is open.
Future<bool> _sidebarShown(WidgetTester tester) async {
  await _pumpWhile(
    tester,
    'the drawer to open or close',
    () =>
        _closeSidebar.evaluate().isNotEmpty &&
        _closeSidebar.hitTestable().evaluate().isEmpty,
  );

  return _closeSidebar.evaluate().isNotEmpty;
}

/// Opens the drawer, where a narrow screen keeps the counts, the sample data
/// and the file's card. A wide screen shows them beside the list.
Future<void> _showSidebar(WidgetTester tester) async {
  if (!_collapsed(tester) || await _sidebarShown(tester)) {
    return;
  }

  await _tap(tester, find.byKey(menuKey));
  await _pumpUntil(tester, _closeSidebar.hitTestable());
}

/// Closes the drawer, which covers the list on a narrow screen.
Future<void> _hideSidebar(WidgetTester tester) async {
  if (!_collapsed(tester) || !await _sidebarShown(tester)) {
    return;
  }

  await _tap(tester, _closeSidebar);
  await _pumpWhile(
    tester,
    'the drawer to close',
    () => _closeSidebar.evaluate().isNotEmpty,
  );
}

Future<void> _waitForCount(
  WidgetTester tester,
  SampleCollection collection,
  String count,
) async {
  await _showSidebar(tester);
  await _pumpUntil(
    tester,
    _textWithKey(countKey(collection), (String text) => text == count),
  );
}

Future<void> _waitForTotal(
  WidgetTester tester,
  bool Function(String total) test,
) => _pumpUntil(tester, _textWithKey(const Key('total'), test));

Future<void> _tap(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.pump();
  // A form or a drawer on its way out still covers what is under it for a
  // few frames.
  await _pumpUntil(tester, finder.hitTestable());
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

/// The line of the file's card that says whether the file is encrypted.
Finder _fileKind(bool encrypted) => find.byWidgetPredicate(
  (Widget widget) =>
      widget is Text &&
      (widget.data?.endsWith(encrypted ? ', encrypted' : ', not encrypted') ??
          false),
);

/// Starts the app on [screen], or on the device's own screen when that is
/// `null`.
Future<void> _startApp(
  WidgetTester tester,
  String directory,
  _Screen? screen,
) async {
  // The test runner without a window lays the app out at this size too; on a
  // desktop it is the window's own.
  if (screen != null) {
    tester.view.physicalSize = screen.size * screen.ratio;
    tester.view.devicePixelRatio = screen.ratio;
    addTearDown(tester.view.reset);
  }

  // A phone's own keyboard would come up at every field and move the form
  // under the finger, so the test types through the test's input instead.
  if (_onDevice) {
    tester.testTextInput.register();
    addTearDown(tester.testTextInput.unregister);
  }

  await tester.pumpWidget(SampleApp(directory: directory));
  await _pumpUntil(tester, find.byKey(menuKey));
}

/// Presses a button of the list's view, with the drawer out of the way.
Future<void> _tapInList(WidgetTester tester, Finder finder) async {
  await _hideSidebar(tester);
  await _tap(tester, finder);
}

/// Presses a button of the sidebar, opening the drawer first if it is one.
Future<void> _tapInSidebar(WidgetTester tester, Finder finder) async {
  await _showSidebar(tester);
  await _tap(tester, finder);
}

Future<void> _applyFilter(WidgetTester tester, String filter) async {
  await _hideSidebar(tester);
  await _type(tester, find.byKey(const Key('filter')), filter);
  await _tap(tester, find.text('Apply'));
}

Future<void> _insertSampleData(WidgetTester tester) async {
  await _showSidebar(tester);
  await _type(tester, find.byKey(const Key('seed')), '7');
  await _tap(tester, find.text('Insert sample data'));
  await _pumpUntil(tester, find.text('Inserted 4,020 objects'));
}

Future<void> _openCollection(
  WidgetTester tester,
  SampleCollection collection,
) async {
  await _tapInSidebar(tester, find.widgetWithText(PlListItem, collection.name));
  await _pumpUntil(tester, find.byKey(ValueKey<SampleCollection>(collection)));
  // A drawer closes by itself once a collection is chosen.
  await _pumpWhile(
    tester,
    'the drawer to close',
    () => _closeSidebar.evaluate().isNotEmpty,
  );
}

/// Waits for the file's card to say what kind of file it is.
Future<void> _waitForFileKind(
  WidgetTester tester, {
  required bool encrypted,
}) async {
  await _showSidebar(tester);
  await _pumpUntil(tester, _fileKind(encrypted));
}

/// Makes the file again through its card, encrypted or plain.
Future<void> _makeFile(WidgetTester tester, {required bool encrypted}) async {
  await _showSidebar(tester);
  await _tap(tester, find.text(encrypted ? 'Encrypted' : 'Plain'));
  await _pumpUntil(
    tester,
    find.text(encrypted ? 'Make an encrypted file?' : 'Make a plain file?'),
  );
  await _tap(tester, find.text(encrypted ? 'Encrypt' : 'Make plain'));
  await _waitForFileKind(tester, encrypted: encrypted);
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  final _Screen? screen = _onDevice ? null : _window;

  for (final bool encrypted in <bool>[false, true]) {
    group(
      encrypted ? 'an encrypted file' : 'a plain file',
      () => _scenario(encrypted: encrypted, screen: screen),
    );
  }

  if (!_onDevice) {
    group(
      "a plain file on a phone's screen",
      () => _scenario(encrypted: false, screen: _phone),
    );
  }
}

void _scenario({required bool encrypted, required _Screen? screen}) {
  late Directory directory;

  setUpAll(() {
    directory = Directory.systemTemp.createTempSync('darudb_sample_app');
  });

  tearDownAll(() {
    directory.deleteSync(recursive: true);
  });

  testWidgets('starts with an empty database, plain or made encrypted', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path, screen);
    await _waitForFileKind(tester, encrypted: false);

    if (encrypted) {
      await _makeFile(tester, encrypted: true);
    }

    for (final SampleCollection collection in SampleCollection.values) {
      await _waitForCount(tester, collection, '0');
    }

    expect(find.text('No objects'), findsOneWidget);
  });

  testWidgets('inserts sample data in bulk, and continues its numbers', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path, screen);
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
    await _startApp(tester, directory.path, screen);
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
    await _startApp(tester, directory.path, screen);
    await _tapInList(tester, find.text('Add object'));
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
    await _tapInList(tester, find.text('Edit'));
    await _pumpUntil(tester, find.byKey(const Key('save')));
    await _type(tester, find.byKey(fieldKey('age')), '34');
    await _tap(tester, find.byKey(const Key('save')));
    await _pumpUntil(tester, find.textContaining('Updated people'));
    await _pumpUntil(tester, find.text('34'));

    // A second person with the same nickname: the unique index refuses it.
    await _tapInList(tester, find.text('Add object'));
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

    await _tapInList(tester, find.text('Delete'));
    await _pumpUntil(tester, find.textContaining('Delete people'));
    await _tap(tester, find.text('Delete').last);
    await _waitForTotal(tester, (String total) => total == '0');
    await _waitForCount(tester, SampleCollection.people, '2,000');
  });

  testWidgets('adds an organization under a key of its own', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path, screen);
    await _openCollection(tester, SampleCollection.organizations);
    await _tapInList(tester, find.text('Add object'));
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
    await _startApp(tester, directory.path, screen);
    await _tapInSidebar(tester, find.text('Check'));
    await _pumpUntil(tester, find.text('The file is intact'));
    await _tapInSidebar(tester, find.text('Compact'));
    await _pumpUntil(tester, find.text('Compacted'));
  });

  testWidgets('keeps what it committed when the app starts again', (
    WidgetTester tester,
  ) async {
    await _startApp(tester, directory.path, screen);
    // An encrypted file opens again with the sample's password.
    await _waitForFileKind(tester, encrypted: encrypted);
    await _waitForCount(tester, SampleCollection.organizations, '41');
    await _waitForCount(tester, SampleCollection.people, '2,000');
    await _waitForCount(tester, SampleCollection.posts, '6,000');
  });

  testWidgets('resets the database', (WidgetTester tester) async {
    await _startApp(tester, directory.path, screen);
    await _tapInSidebar(tester, find.text('Reset'));
    await _pumpUntil(tester, find.text('Delete every object?'));
    await _tap(tester, find.text('Reset').last);
    await _pumpUntil(tester, find.text('The database is empty'));

    for (final SampleCollection collection in SampleCollection.values) {
      await _waitForCount(tester, collection, '0');
    }

    // A reset keeps the kind of file, and the card turns it into the other.
    await _showSidebar(tester);
    expect(_fileKind(encrypted), findsOneWidget);
    await _makeFile(tester, encrypted: !encrypted);
  });
}
