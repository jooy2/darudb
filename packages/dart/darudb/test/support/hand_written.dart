// What `darudb_generator` writes for two classes, written by hand, so that
// the runtime is tested apart from the generator.
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

final class Address {
  const Address({required this.city, this.zip});

  final String city;
  final String? zip;

  @override
  bool operator ==(Object other) =>
      other is Address && other.city == city && other.zip == zip;

  @override
  int get hashCode => Object.hash(city, zip);
}

const addressSchema = EmbeddedSchema<Address>(
  fields: [
    FieldSpec('city', StringKind()),
    FieldSpec('zip', StringKind(), optional: true),
  ],
  writeField: _writeAddress,
  read: _readAddress,
);

void _writeAddress(Address object, int slot, FieldSink sink) {
  switch (slot) {
    case 0:
      sink.string(object.city);
    case 1:
      sink.string(object.zip);
  }
}

Address _readAddress(FieldSource source) {
  String? city;
  String? zip;

  while (source.next()) {
    switch (source.slot) {
      case 0:
        city = source.string();
      case 1:
        zip = source.stringOrNull();
    }
  }

  return Address(city: city!, zip: zip);
}

final class AddressFields extends EmbeddedField {
  const AddressFields(super.path);

  StringField get city => StringField([...path, 'city']);
  StringField get zip => StringField([...path, 'zip']);
}

final class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.score = 0.5,
    this.tags = const [],
    this.photo,
    this.address,
    this.friend,
  });

  final int? id;
  final String name;
  final String? email;
  final int age;
  final double score;
  final List<String> tags;
  final Uint8List? photo;
  final Address? address;
  final Link<User>? friend;

  User copyWith({int? id, String? name, int? age}) => User(
    id: id ?? this.id,
    name: name ?? this.name,
    email: email,
    age: age ?? this.age,
    score: score,
    tags: tags,
    photo: photo,
    address: address,
    friend: friend,
  );
}

const userSchema = CollectionSchema<User, UserQuery, int>(
  name: 'users',
  autoKey: true,
  fields: [
    FieldSpec('id', IntKind()),
    FieldSpec('name', StringKind()),
    FieldSpec('email', StringKind(), optional: true, unique: true),
    FieldSpec('age', IntKind(), defaultValue: 0, index: true),
    FieldSpec('score', FloatKind(), defaultValue: 0.5),
    FieldSpec('tags', ListKind(StringKind()), index: true),
    FieldSpec('photo', BytesKind(), optional: true),
    FieldSpec('address', ObjectKind(addressSchema), optional: true),
    FieldSpec('friend', LinkKind('users'), optional: true),
  ],
  writeField: _writeUser,
  read: _readUser,
  query: UserQuery.new,
);

void _writeUser(User object, int slot, FieldSink sink) {
  switch (slot) {
    case 0:
      sink.int64(object.id);
    case 1:
      sink.string(object.name);
    case 2:
      sink.string(object.email);
    case 3:
      sink.int64(object.age);
    case 4:
      sink.float(object.score);
    case 5:
      sink.list(object.tags);
    case 6:
      sink.bytes(object.photo);
    case 7:
      sink.object(object.address, addressSchema);
    case 8:
      sink.link(object.friend);
  }
}

User _readUser(FieldSource source) {
  int? id;
  String? name;
  String? email;
  int? age;
  double? score;
  List<String>? tags;
  Uint8List? photo;
  Address? address;
  Link<User>? friend;

  while (source.next()) {
    switch (source.slot) {
      case 0:
        id = source.int64();
      case 1:
        name = source.string();
      case 2:
        email = source.stringOrNull();
      case 3:
        age = source.int64();
      case 4:
        score = source.float();
      case 5:
        tags = source.list<String>();
      case 6:
        photo = source.bytesOrNull();
      case 7:
        address = source.objectOrNull(addressSchema);
      case 8:
        friend = source.linkOrNull<User>();
    }
  }

  return User(
    id: id,
    name: name!,
    email: email,
    age: age!,
    score: score!,
    tags: tags!,
    photo: photo,
    address: address,
    friend: friend,
  );
}

final class UserLink extends LinkField {
  const UserLink(super.path);

  IntField get id => IntField([...path, 'id']);
  StringField get name => StringField([...path, 'name']);
  IntField get age => IntField([...path, 'age']);
}

final class UserQuery extends QueryBuilder<User> {
  UserQuery();

  IntField get id => const IntField(['id']);
  StringField get name => const StringField(['name']);
  StringField get email => const StringField(['email']);
  IntField get age => const IntField(['age']);
  FloatField get score => const FloatField(['score']);
  StringListField get tags => const StringListField(['tags']);
  BytesField get photo => const BytesField(['photo']);
  AddressFields get address => const AddressFields(['address']);
  UserLink get friend => const UserLink(['friend']);
}
