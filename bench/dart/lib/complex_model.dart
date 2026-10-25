import 'package:darudb/darudb.dart';

part 'complex_model.g.dart';

/// The complex suite's collections, as a Dart application declares them: a
/// link from each person to an organization and from each post to its
/// author, an embedded address, and lists of tags with an index on each.
@Collection('organizations')
class Organization {
  const Organization({
    required this.code,
    required this.name,
    required this.kind,
    required this.country,
    required this.founded,
  });

  @PrimaryKey()
  final String code;
  final String name;
  @Index()
  final String kind;
  @Index()
  final String country;
  final int founded;
}

@Embedded()
class Address {
  const Address({required this.city, required this.zip});

  final String city;
  final String zip;
}

@Collection('people')
class Person {
  const Person({
    this.id,
    required this.name,
    required this.email,
    required this.age,
    required this.country,
    required this.address,
    required this.score,
    required this.active,
    required this.joinedAt,
    required this.organization,
    required this.tags,
  });

  final int? id;
  final String name;
  @Unique()
  final String email;
  @Index()
  final int age;
  @Index()
  final String country;
  final Address address;
  final double score;
  final bool active;
  @Index()
  final int joinedAt;
  @Index()
  final Link<Organization> organization;
  @Index()
  final List<String> tags;
}

@Collection('posts')
class Post {
  const Post({
    this.id,
    required this.author,
    required this.title,
    required this.body,
    required this.likes,
    required this.language,
    required this.createdAt,
    required this.pinned,
    required this.tags,
  });

  final int? id;
  @Index()
  final Link<Person> author;
  final String title;
  final String body;
  @Index()
  final int likes;
  @Index()
  final String language;
  @Index()
  final int createdAt;
  final bool pinned;
  @Index()
  final List<String> tags;
}
