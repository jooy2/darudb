/// The complex suite's collections as a Realm model, from which
/// `build_runner` generates the classes: a link from each person to an
/// organization and from each post to its author, the posts of a person as a
/// backlink, since Realm cannot index a link, an embedded address, and lists
/// of tags, which Realm does not index.
library;

import 'package:realm_dart/realm.dart';

part 'complex_model.realm.dart';

// The generator names each class from these, without the underscore.
// ignore_for_file: unused_element

@RealmModel()
class _Organization {
  @PrimaryKey()
  late String code;
  late String name;
  @Indexed()
  late String kind;
  @Indexed()
  late String country;
  late int founded;
}

@RealmModel(ObjectType.embeddedObject)
class _Address {
  late String city;
  late String zip;
}

@RealmModel()
class _Person {
  @PrimaryKey()
  late int id;
  late String name;
  @Indexed()
  late String email;
  @Indexed()
  late int age;
  @Indexed()
  late String country;
  _Address? address;
  late double score;
  late bool active;
  @Indexed()
  late int joinedAt;
  _Organization? organization;
  late List<String> tags;
  @Backlink(#author)
  late Iterable<_Post> posts;
}

@RealmModel()
class _Post {
  @PrimaryKey()
  late int id;
  _Person? author;
  late String title;
  late String body;
  @Indexed()
  late int likes;
  @Indexed()
  late String language;
  @Indexed()
  late int createdAt;
  late bool pinned;
  late List<String> tags;
}
