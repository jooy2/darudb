import 'package:build/build.dart';
import 'package:build_test/build_test.dart';
import 'package:darudb_generator/builder.dart';
import 'package:test/test.dart';

/// Runs the builder on a library holding [body], and returns what it wrote
/// and the errors it reported.
Future<(String?, List<String>)> generate(String body) async {
  // The annotations are the real ones, from the `darudb` package this one
  // depends on for its tests.
  final readerWriter = TestReaderWriter(
    rootPackage: 'app',
    flattenOutput: true,
  );

  await readerWriter.testing.loadIsolateSources();

  final result = await testBuilder(
    darudbBuilder(BuilderOptions.empty),
    {
      'app|lib/model.dart':
          '''
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'model.g.dart';

$body
''',
    },
    rootPackage: 'app',
    readerWriter: readerWriter,
    flattenOutput: true,
  );
  final part = result.outputs
      .where((id) => id.path.endsWith('.darudb.g.part'))
      .firstOrNull;
  final written = part == null
      ? null
      : result.readerWriter.testing.readString(part);

  return (written, result.errors.toList());
}

void main() {
  test('a collection gets its schema, codec, query and copyWith', () async {
    final (written, errors) = await generate('''
@Collection('users')
class User {
  const User({this.id, required this.name, this.age = 0, this.tags = const []});

  final int? id;
  @Name('full_name')
  final String name;
  @Index()
  final int age;
  final List<String> tags;
}
''');

    expect(errors, isEmpty);
    expect(
      written,
      contains('const userSchema = CollectionSchema<User, UserQuery, int>('),
    );
    expect(written, contains("FieldSpec('full_name', StringKind())"));
    expect(
      written,
      contains("FieldSpec('age', IntKind(), defaultValue: 0, index: true)"),
    );
    expect(
      written,
      contains("FieldSpec('tags', ListKind(StringKind()), defaultValue: [])"),
    );
    expect(
      written,
      contains("StringField get name => const StringField(['full_name']);"),
    );
    expect(written, contains('extension UserCopyWith on User'));
    expect(written, contains('final class UserLink extends LinkField'));
  });

  Future<void> refused(String body, String message) async {
    final (_, errors) = await generate(body);

    expect(errors.join('\n'), contains(message));
  }

  test('a collection without a key needs `int? id`', () async {
    await refused('''
@Collection()
class Note {
  const Note({required this.text});

  final String text;
}
''', 'needs a field `final int? id`');
  });

  test('a field that is not final is refused', () async {
    await refused('''
@Collection()
class Note {
  Note({this.id, required this.text});

  final int? id;
  String text;
}
''', 'is not final');
  });

  test('a type the database does not store is refused', () async {
    await refused('''
@Collection()
class Note {
  const Note({this.id, required this.at});

  final int? id;
  final DateTime at;
}
''', 'is not a type the database stores');
  });

  test('a nullable field with a default is refused', () async {
    await refused('''
@Collection()
class Note {
  const Note({this.id, this.text = 'x'});

  final int? id;
  final String? text;
}
''', 'is nullable and has a default');
  });

  test('two primary keys are refused', () async {
    await refused('''
@Collection()
class Note {
  const Note({required this.a, required this.b});

  @PrimaryKey()
  final String a;
  @PrimaryKey()
  final String b;
}
''', 'more than one `@PrimaryKey()`');
  });

  test('an index inside an embedded object is refused', () async {
    await refused('''
@Embedded()
class Place {
  const Place({required this.city});

  @Index()
  final String city;
}
''', 'no index reaches inside one');
  });

  test('a field the constructor does not take is refused', () async {
    await refused('''
@Collection()
class Note {
  const Note({this.id});

  final int? id;
  final String text = '';
}
''', 'does not take `text`');
  });
}
