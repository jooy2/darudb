/// The objects of the benchmark as a Realm model, from which
/// `build_runner` generates the class `Person`.
library;

import 'package:realm_dart/realm.dart';

part 'model.realm.dart';

@RealmModel()
// The generator names the class from this one, without the underscore.
// ignore: unused_element
class _Person {
  @PrimaryKey()
  late int id;
  late String name;
  @Indexed()
  late String email;
  @Indexed()
  late int age;
  late String city;
  late double score;
}
