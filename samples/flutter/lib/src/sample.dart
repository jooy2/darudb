// The sample data: organizations, people and posts in nine languages, made
// with `randino` from a seed, by the rules of `samples/node/core/sample.ts`.
//
// A run first fills pools from the seed, a few hundred names, words,
// sentences and places for each language, since `randino` writes full
// sentences slowly, and each object is then a handful of draws from those
// pools. Each object draws from a seed of its own, made from the run's seed
// and the object's number, so object 1042 of seed 7 is the same object
// whichever batch makes it. The numbers continue from the objects already in
// the file, which keeps the unique fields unique. `randino` draws differently
// in Dart than in JavaScript, so the two samples do not make the same names
// from the same seed.
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:randino/randino.dart';

import 'package:darudb_sample/src/model.dart';
import 'package:darudb_sample/src/random.dart';

const List<String> sampleLanguages = <String>[
  'ko',
  'en',
  'ja',
  'zh',
  'de',
  'es',
  'it',
  'ru',
  'vi',
];

/// How often each of [sampleLanguages] comes up: Korean and English most.
const List<int> _languageWeights = <int>[5, 5, 2, 2, 1, 1, 1, 1, 1];

/// The languages `randino` has places for, down to a city; the rest get a
/// country.
const List<String> _placeLanguages = <String>['ko', 'en'];

/// The languages whose sentences follow each other without a space.
const List<String> _unspacedLanguages = <String>['ja', 'zh'];

const int peoplePerOrganization = 50;
const int postsPerPerson = 3;

/// What every sample organization's code starts with.
const String organizationPrefix = 'ORG-';

/// What a pool or a kind of object draws from, mixed into its seed.
abstract final class _Stream {
  static const int names = 1;
  static const int nicknames = 2;
  static const int words = 3;
  static const int titles = 4;
  static const int sentences = 5;
  static const int organizations = 6;
  static const int places = 7;
  static const int countries = 8;
  static const int organization = 11;
  static const int person = 12;
  static const int post = 13;
}

abstract final class _PoolSize {
  static const int names = 400;
  static const int nicknames = 1500;
  static const int words = 120;
  static const int titles = 150;
  static const int sentences = 150;
  static const int organizations = 200;
  static const int places = 400;
  static const int countries = 120;
}

/// The first of January 2020, and the six years after it, in milliseconds.
final int _epochMs = DateTime.utc(2020).millisecondsSinceEpoch;
final int _spanMs = DateTime.utc(2026).millisecondsSinceEpoch - _epochMs;

/// How many of each a run makes.
final class Plan {
  const Plan(this.people);

  final int people;

  int get organizations =>
      (people + peoplePerOrganization - 1) ~/ peoplePerOrganization;

  int get posts => people * postsPerPerson;

  int get total => organizations + people + posts;
}

/// An organization's code: its number, padded so that codes sort as numbers
/// do.
String organizationCode(int number) =>
    '$organizationPrefix${number.toString().padLeft(6, '0')}';

/// A person a post can be by: its key, and the language its posts are in.
final class Author {
  const Author(this.key, this.language);

  final int key;
  final String language;
}

final class _Pools {
  const _Pools({
    required this.names,
    required this.words,
    required this.titles,
    required this.sentences,
    required this.organizations,
    required this.places,
  });

  final List<NameDetail> names;
  final List<String> words;
  final List<String> titles;
  final List<String> sentences;
  final List<OrganizationDetail> organizations;
  final List<Place> places;
}

final class SampleData {
  SampleData(this.seed)
    : _nicknames = randNickname(
        language: WordLanguage.en,
        count: _PoolSize.nicknames,
        unique: true,
        random: SeededRandom(seedOf(<int>[seed, _Stream.nicknames, 0])),
      ) {
    for (int index = 0; index < sampleLanguages.length; index += 1) {
      _pools[sampleLanguages[index]] = _fill(sampleLanguages[index], index);
    }
  }

  final int seed;
  final List<String> _nicknames;
  final Map<String, _Pools> _pools = <String, _Pools>{};

  /// Organization number [number].
  Organization organization(int number) {
    final Draw draw = Draw(seedOf(<int>[seed, _Stream.organization, number]));
    final String language = _language(draw);
    final OrganizationDetail made = draw.pick(_poolsOf(language).organizations);

    return Organization(
      code: organizationCode(number),
      name: made.organization,
      kind: made.type.name,
      industry: made.industry?.name,
      language: language,
      founded: draw.between(1950, 2025),
    );
  }

  /// Person number [number], who may belong to one of [organizations], given
  /// by code.
  Person person(int number, List<String> organizations) {
    final Draw draw = Draw(seedOf(<int>[seed, _Stream.person, number]));
    final String language = _language(draw);
    final _Pools pools = _poolsOf(language);
    final NameDetail name = draw.pick(pools.names);
    final String nickname = '${draw.pick(_nicknames)}$number';

    return Person(
      name: name.native,
      nickname: nickname,
      email: draw.chance(0.8) ? '${nickname.toLowerCase()}@example.com' : null,
      age: randAge(minAge: 14, maxAge: 90, random: draw.random).first,
      gender: name.gender.name,
      language: language,
      location: draw.chance(0.95) ? draw.pick(pools.places) : null,
      organization: organizations.isNotEmpty && draw.chance(0.85)
          ? Link<Organization>(draw.pick(organizations))
          : null,
      tags: draw.picks(pools.words, draw.below(5)),
      active: draw.chance(0.9),
      score: (draw.random.nextDouble() * 1000).round() / 10,
      color: Uint8List.fromList(<int>[
        draw.below(256),
        draw.below(256),
        draw.below(256),
      ]),
      joinedAt: _epochMs + draw.below(_spanMs),
    );
  }

  /// Post number [number], by one of [authors].
  Post post(int number, List<Author> authors) {
    final Draw draw = Draw(seedOf(<int>[seed, _Stream.post, number]));
    final Author author = draw.pick(authors);
    final _Pools pools = _poolsOf(author.language);
    final String separator = _unspacedLanguages.contains(author.language)
        ? ''
        : ' ';
    final double likes = draw.random.nextDouble();

    return Post(
      author: Link<Person>(author.key),
      title: draw.pick(pools.titles),
      body: draw.picks(pools.sentences, draw.between(1, 3)).join(separator),
      language: author.language,
      tags: draw.chance(0.6)
          ? draw.picks(pools.words, draw.between(1, 3))
          : null,
      likes: (likes * likes * likes * 500).floor(),
      pinned: draw.chance(0.02),
      createdAt: _epochMs + draw.below(_spanMs),
    );
  }

  _Pools _poolsOf(String language) {
    final _Pools? pools = _pools[language];

    if (pools == null) {
      throw StateError('no sample pools for the language $language');
    }

    return pools;
  }

  SeededRandom _stream(int stream, int language) =>
      SeededRandom(seedOf(<int>[seed, stream, language]));

  String _language(Draw draw) =>
      draw.weighted(sampleLanguages, _languageWeights);

  _Pools _fill(String language, int index) {
    final NameLanguage nameLanguage = NameLanguage.values.byName(language);
    final WordLanguage wordLanguage = WordLanguage.values.byName(language);

    return _Pools(
      names: randNameDetails(
        language: nameLanguage,
        count: _PoolSize.names,
        random: _stream(_Stream.names, index),
      ),
      words: randWord(
        language: wordLanguage,
        count: _PoolSize.words,
        unique: true,
        random: _stream(_Stream.words, index),
      ),
      titles: randSentence(
        language: wordLanguage,
        count: _PoolSize.titles,
        shape: SentenceShape.simple,
        random: _stream(_Stream.titles, index),
      ),
      sentences: randSentence(
        language: wordLanguage,
        count: _PoolSize.sentences,
        random: _stream(_Stream.sentences, index),
      ),
      organizations: randOrganizationDetails(
        language: wordLanguage,
        count: _PoolSize.organizations,
        random: _stream(_Stream.organizations, index),
      ),
      places: _places(language, wordLanguage, index),
    );
  }

  List<Place> _places(String language, WordLanguage wordLanguage, int index) {
    if (_placeLanguages.contains(language)) {
      return <Place>[
        for (final LocationDetail place in randLocationDetails(
          language: LocationLanguage.values.byName(language),
          level: LocationLevel.city,
          count: _PoolSize.places,
          random: _stream(_Stream.places, index),
        ))
          Place(country: place.country, region: place.region, city: place.city),
      ];
    }

    return <Place>[
      for (final String country in randCountry(
        language: wordLanguage,
        count: _PoolSize.countries,
        unique: true,
        random: _stream(_Stream.countries, index),
      ))
        Place(country: country),
    ];
  }
}
