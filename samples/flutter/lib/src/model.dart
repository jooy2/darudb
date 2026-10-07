// The sample's collections, the same three the Node.js sample declares in
// `samples/node/core/schema.ts`, with the same names and fields: an
// organization under a string key of its own, people under the automatic
// `id` with an embedded place, a link, a list and bytes, and posts linking
// to people. `dart run build_runner build` writes `model.g.dart`, with the
// schema constants and query builders `store.dart` uses.
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'model.g.dart';

@Collection('organizations')
class Organization {
  const Organization({
    required this.code,
    required this.name,
    required this.kind,
    this.industry,
    required this.language,
    required this.founded,
  });

  @PrimaryKey()
  final String code;
  @Index()
  final String name;
  @Index()
  final String kind;
  final String? industry;
  final String language;
  @Index()
  final int founded;
}

@Embedded()
class Place {
  const Place({required this.country, this.region, this.city});

  final String country;
  final String? region;
  final String? city;
}

@Collection('people')
class Person {
  const Person({
    this.id,
    required this.name,
    required this.nickname,
    this.email,
    required this.age,
    required this.gender,
    required this.language,
    this.location,
    this.organization,
    this.tags = const <String>[],
    this.active = true,
    this.score = 0.0,
    required this.color,
    required this.joinedAt,
  });

  final int? id;
  @Index()
  final String name;
  @Unique()
  final String nickname;
  @Unique()
  final String? email;
  @Index()
  final int age;
  final String gender;
  @Index()
  final String language;
  final Place? location;
  @Index()
  final Link<Organization>? organization;
  @Index()
  final List<String> tags;
  final bool active;
  final double score;
  final Uint8List color;
  @Index()
  final int joinedAt;
}

@Collection('posts')
class Post {
  const Post({
    this.id,
    required this.author,
    required this.title,
    required this.body,
    required this.language,
    this.tags,
    this.likes = 0,
    this.pinned = false,
    required this.createdAt,
  });

  final int? id;
  @Index()
  final Link<Person> author;
  final String title;
  final String body;
  @Index()
  final String language;
  @Index()
  final List<String>? tags;
  @Index()
  final int likes;
  final bool pinned;
  @Index()
  final int createdAt;
}

/// The schema the app opens its database with.
const Schema sampleSchema = Schema(1, [
  organizationSchema,
  personSchema,
  postSchema,
]);
