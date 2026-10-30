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

  test('an embedded object gets its schema, its fields and copyWith', () async {
    final (written, errors) = await generate('''
@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  final String? zip;
}
''');

    expect(errors, isEmpty);
    expect(written, contains('const addressSchema = EmbeddedSchema<Address>('));
    // The field object takes the schema, which its `set` writes with.
    expect(
      written,
      contains(
        'final class AddressFields extends EmbeddedField<Address> {\n'
        '  const AddressFields(List<String> path) : super(path, addressSchema);',
      ),
    );
    expect(written, contains('extension AddressCopyWith on Address'));
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
  const Note({this.id, required this.wait});

  final int? id;
  final Duration wait;
}
''', 'is not a type the database stores');
  });

  test('a List of an embedded class is a list of embedded objects', () async {
    final (written, errors) = await generate('''
@Embedded()
class Stop {
  const Stop({required this.city});

  final String city;
}

@Collection('trips')
class Trip {
  const Trip({this.id, this.stops = const []});

  final int? id;
  final List<Stop> stops;
}
''');

    expect(errors, isEmpty);
    expect(
      written,
      contains(
        "FieldSpec('stops', ListKind(ObjectKind(stopSchema)), defaultValue: [])",
      ),
    );
    expect(written, contains('sink.objectList('));
    expect(written, contains('source.objectList(stopSchema)'));
    expect(
      written,
      contains("StopListFields get stops => const StopListFields(['stops']);"),
    );
    expect(
      written,
      contains('final class StopListFields extends EmbeddedListField<Stop>'),
    );
  });

  test(
    '@IndexOn declares an index by the names the collection stores',
    () async {
      final (written, errors) = await generate('''
@Embedded()
class Home {
  const Home({this.city});

  @Name('town')
  final String? city;
}

@Collection('people')
@IndexOn(['last', 'first'], unique: true)
@IndexOn(['home.city'])
class Person {
  const Person({this.id, required this.last, this.first, this.home});

  final int? id;
  final String last;
  @Name('given')
  final String? first;
  final Home? home;
}
''');

      expect(errors, isEmpty);
      expect(
        written,
        contains(
          "  indexes: [\n    IndexSpec(['last', 'given'], unique: true),\n"
          "    IndexSpec(['home.town']),\n  ],",
        ),
      );
    },
  );

  test('@IndexOn on a field the class lacks is refused', () async {
    await refused('''
@Collection()
@IndexOn(['nope'])
class Note {
  const Note({this.id});

  final int? id;
}
''', 'names `nope`, which the class does not have');
  });

  test('a DateTime field is a date, alone or in a list', () async {
    final (written, errors) = await generate('''
@Collection('events')
class Event {
  const Event({this.id, required this.at, this.ends, this.seen = const []});

  final int? id;
  @Index()
  final DateTime at;
  final DateTime? ends;
  final List<DateTime> seen;
}
''');

    expect(errors, isEmpty);
    expect(written, contains("FieldSpec('at', DateKind(), index: true)"));
    expect(written, contains("FieldSpec('ends', DateKind(), optional: true)"));
    expect(
      written,
      contains("FieldSpec('seen', ListKind(DateKind()), defaultValue: [])"),
    );
    expect(written, contains("DateField get at => const DateField(['at']);"));
    expect(written, contains('source.dateOrNull()'));
    expect(written, contains('sink.date('));
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
