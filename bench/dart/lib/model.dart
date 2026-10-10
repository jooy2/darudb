import 'package:darudb/darudb.dart';

part 'model.g.dart';

/// The objects of DaruDB's collection, as a Dart application declares them.
@Collection('people')
class Person {
  const Person({
    this.id,
    required this.name,
    required this.email,
    required this.age,
    required this.city,
    required this.score,
  });

  final int? id;
  final String name;
  @Unique()
  final String email;
  @Index()
  final int age;
  final String city;
  final double score;
}
