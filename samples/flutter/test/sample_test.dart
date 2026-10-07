import 'package:flutter_test/flutter_test.dart';

import 'package:darudb_sample/src/model.dart';
import 'package:darudb_sample/src/random.dart';
import 'package:darudb_sample/src/sample.dart';

void main() {
  test('one seed and number make the same person, whichever run makes it', () {
    final Person first = SampleData(7).person(1042, <String>['ORG-000001']);
    final Person again = SampleData(7).person(1042, <String>['ORG-000001']);

    expect(again.name, first.name);
    expect(again.nickname, first.nickname);
    expect(again.age, first.age);
    expect(again.color, first.color);
    expect(first.nickname, endsWith('1042'));
  });

  test('the numbers keep nicknames, emails and codes unique', () {
    final SampleData data = SampleData(1);
    final Set<String> nicknames = <String>{
      for (int number = 1; number <= 2000; number += 1)
        data.person(number, const <String>[]).nickname,
    };

    expect(nicknames, hasLength(2000));
    expect(data.organization(12).code, 'ORG-000012');
  });

  test('a plan follows from the number of people', () {
    const Plan plan = Plan(1001);

    expect(plan.organizations, 21);
    expect(plan.posts, 3003);
    expect(plan.total, 21 + 1001 + 3003);
  });

  test('the seeded generator draws what the Node.js sample\'s draws', () {
    // `seedOf(7, 12, 1)` and the first draws of `mulberry32` of it, from
    // `samples/node/core/random.ts`.
    final SeededRandom random = SeededRandom(seedOf(<int>[7, 12, 1]));

    expect(seedOf(<int>[7, 12, 1]), 2935329783);
    expect(random.nextDouble(), 0.3884077803231776);
    expect(random.nextDouble(), 0.01347192726098001);
    expect(random.nextDouble(), 0.02291405270807445);
  });
}
