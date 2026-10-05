// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'main.dart';

// **************************************************************************
// DaruGenerator
// **************************************************************************

/// The collection `users` of [User] objects, for `Schema` and
/// `txn.collection`.
const userSchema = CollectionSchema<User, UserQuery, int>(
  name: 'users',
  autoKey: true,
  fields: [
    FieldSpec('id', IntKind()),
    FieldSpec('name', StringKind()),
    FieldSpec('email', StringKind(), optional: true, unique: true),
    FieldSpec('age', IntKind(), defaultValue: 0, index: true),
  ],
  writeField: _$writeUser,
  read: _$readUser,
  query: UserQuery.new,
);

void _$writeUser(User object, int slot, FieldSink sink) {
  switch (slot) {
    case 0:
      sink.int64(object.id);
    case 1:
      sink.string(object.name);
    case 2:
      sink.string(object.email);
    case 3:
      sink.int64(object.age);
  }
}

User _$readUser(FieldSource source) {
  int? field0;
  String? field1;
  String? field2;
  int? field3;

  while (source.next()) {
    switch (source.slot) {
      case 0:
        field0 = source.int64OrNull();
      case 1:
        field1 = source.string();
      case 2:
        field2 = source.stringOrNull();
      case 3:
        field3 = source.int64();
    }
  }

  return User(id: field0, name: field1!, email: field2, age: field3!);
}

/// A query on the collection `users`, with a field for each field of
/// [User].
final class UserQuery extends QueryBuilder<User> {
  UserQuery();

  IntField get id => const IntField(['id']);
  StringField get name => const StringField(['name']);
  StringField get email => const StringField(['email']);
  IntField get age => const IntField(['age']);
}

/// A link to an object of [User], whose fields a query reads through
/// the link.
final class UserLink extends LinkField {
  const UserLink(super.path);

  IntField get id => IntField([...path, 'id']);
  StringField get name => StringField([...path, 'name']);
  StringField get email => StringField([...path, 'email']);
  IntField get age => IntField([...path, 'age']);
}

/// Copies of [User] with some fields changed.
extension UserCopyWith on User {
  /// A copy with the fields given changed. A field left out, or given null,
  /// keeps its value.
  User copyWith({int? id, String? name, String? email, int? age}) => User(
    id: id ?? this.id,
    name: name ?? this.name,
    email: email ?? this.email,
    age: age ?? this.age,
  );
}
