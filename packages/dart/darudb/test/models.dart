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

// Two versions of one collection. The embedded object gains a field declared
// before the one it had, so in a file the first version made, the stored
// field ids of the newer class's embedded object differ from its slots.
@Embedded()
class SpotV1 {
  const SpotV1({required this.city});

  final String city;
}

@Collection('places')
class PlaceV1 {
  const PlaceV1({this.id, this.spot});

  final int? id;
  final SpotV1? spot;
}

@Embedded()
class Spot {
  const Spot({this.zip, required this.city});

  final String? zip;
  final String city;
}

@Collection('places')
class Place {
  const Place({this.id, this.spot});

  final int? id;
  final Spot? spot;
}
