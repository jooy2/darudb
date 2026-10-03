// Two versions of one collection, as an application's schema changes across
// a migration, written as the generator writes them.
import 'package:darudb/darudb.dart';

final class NoteV1 {
  const NoteV1({this.id, required this.title, required this.body});

  final int? id;
  final String title;
  final String body;
}

const noteV1Schema = CollectionSchema<NoteV1, NoteV1Query, int>(
  name: 'notes',
  autoKey: true,
  fields: [
    FieldSpec('id', IntKind()),
    FieldSpec('title', StringKind()),
    FieldSpec('body', StringKind()),
  ],
  writeField: _writeV1,
  read: _readV1,
  query: NoteV1Query.new,
);

void _writeV1(NoteV1 object, int slot, FieldSink sink) {
  switch (slot) {
    case 0:
      sink.int64(object.id);
    case 1:
      sink.string(object.title);
    case 2:
      sink.string(object.body);
  }
}

NoteV1 _readV1(FieldSource source) {
  int? id;
  String? title;
  String? body;

  while (source.next()) {
    switch (source.slot) {
      case 0:
        id = source.int64();
      case 1:
        title = source.string();
      case 2:
        body = source.string();
    }
  }

  return NoteV1(id: id, title: title!, body: body!);
}

final class NoteV1Query extends QueryBuilder<NoteV1> {
  NoteV1Query();
}

/// Version 2: `title` renamed `heading`, `body` gone, `words` added with a
/// default, which the migration function fills from the old `body`.
final class Note {
  const Note({this.id, required this.heading, this.words = 0});

  final int? id;
  final String heading;
  final int words;
}

const noteSchema = CollectionSchema<Note, NoteQuery, int>(
  name: 'notes',
  autoKey: true,
  fields: [
    FieldSpec('id', IntKind()),
    FieldSpec('heading', StringKind()),
    FieldSpec('words', IntKind(), defaultValue: 0),
  ],
  writeField: _write,
  read: _read,
  query: NoteQuery.new,
);

void _write(Note object, int slot, FieldSink sink) {
  switch (slot) {
    case 0:
      sink.int64(object.id);
    case 1:
      sink.string(object.heading);
    case 2:
      sink.int64(object.words);
  }
}

Note _read(FieldSource source) {
  int? id;
  String? heading;
  int? words;

  while (source.next()) {
    switch (source.slot) {
      case 0:
        id = source.int64();
      case 1:
        heading = source.string();
      case 2:
        words = source.int64();
    }
  }

  return Note(id: id, heading: heading!, words: words!);
}

final class NoteQuery extends QueryBuilder<Note> {
  NoteQuery();

  IntField get words => const IntField(['words']);
}
