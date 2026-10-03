// Classes annotated for the generator, whose generated code the tests use
// the way an application would.
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'models.g.dart';

@Embedded()
class Address {
  const Address({required this.city, this.zip, this.point = const []});

  final String city;
  @Name('postcode')
  final String? zip;
  final List<double> point;
}

@Collection('people')
class Person {
  const Person({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.score = 0.5,
    this.active = true,
    this.tags = const [],
    this.photo,
    this.home,
    this.friend,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  final double score;
  final bool active;
  @Index()
  final List<String> tags;
  final Uint8List? photo;
  final Address? home;
  final Link<Person>? friend;
}

@Collection('posts')
class Post {
  const Post(this.slug, {required this.author, this.readers = const []});

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<Person> author;
  final List<Link<Person>> readers;
}
