/// What every store runs in the complex suite: three collections that link
/// to each other, the objects in them, and the parameters of each row's
/// runs. `bench/README.md` describes the suite, and the other languages'
/// harnesses draw the same objects and parameters from the same hash.
library;

const int organizationCount = 1000;
const int peopleCount = 100000;
const int postCount = 300000;

/// Objects a write transaction of the load inserts.
const int batch = 10000;

const kinds = ['company', 'school', 'hospital', 'agency', 'club'];

const countries = [
  'kr', 'us', 'jp', 'de', 'fr', 'gb', 'cn', 'in', 'br', 'ca', //
  'au', 'es', 'it', 'nl', 'se', 'mx', 'id', 'tr', 'pl', 'vn',
];

const languages = ['en', 'ko', 'ja', 'de', 'fr', 'es'];

const tags = [
  'travel', 'food', 'music', 'sport', 'news', 'tech', 'art', 'books', //
  'games', 'film', 'health', 'science', 'nature', 'design', 'fashion',
  'history', 'photo', 'pets', 'cars', 'money',
];

const words = [
  'able', 'about', 'above', 'across', 'after', 'again', 'air', 'almost', //
  'along', 'always', 'animal', 'answer', 'around', 'away', 'back', 'before',
  'began', 'below', 'between', 'black', 'body', 'book', 'both', 'bring',
  'build', 'call', 'came', 'carry', 'change', 'child', 'city', 'close',
  'color', 'come', 'country', 'cover', 'cross', 'dark', 'day', 'deep',
  'door', 'draw', 'early', 'earth', 'east', 'end', 'even', 'every', 'face',
  'fall', 'family', 'far', 'farm', 'fast', 'field', 'fire', 'first', 'food',
  'foot', 'form', 'found', 'free', 'friend', 'full',
];

const int _mask = 0xFFFFFFFF;

/// A 32-bit integer hash, the same in every language's harness.
int _hash(int value) {
  var x = (((value >> 16) ^ value) * 0x045D9F3B) & _mask;

  x = (((x >> 16) ^ x) * 0x045D9F3B) & _mask;

  return (x >> 16) ^ x;
}

/// The value [salt] draws for the [n]th object or run.
int mix(int n, int salt) => _hash((_hash(n & _mask) + salt) & _mask);

String _pick(List<String> values, int n, int salt) =>
    values[mix(n, salt) % values.length];

List<String> _tagsOf(int count, int first) => [
  for (var at = 0; at < count; at++) tags[(first % 20 + at * 7) % 20],
];

/// The code of the [n]th organization.
String code(int n) => 'org-${n.toString().padLeft(4, '0')}';

final class OrganizationFields {
  OrganizationFields(
    this.code,
    this.name,
    this.kind,
    this.country,
    this.founded,
  );

  final String code;
  final String name;
  final String kind;
  final String country;
  final int founded;
}

/// A person's fields, as the stores that keep no class of their own write
/// them and give them back.
final class PersonFields {
  PersonFields({
    required this.id,
    required this.name,
    required this.email,
    required this.age,
    required this.country,
    required this.city,
    required this.zip,
    required this.score,
    required this.active,
    required this.joinedAt,
    required this.organization,
    required this.tags,
  });

  final int id;
  final String name;
  final String email;
  final int age;
  final String country;
  final String city;
  final String zip;
  final double score;
  final bool active;
  final int joinedAt;
  final String organization;
  final List<String> tags;
}

final class PostFields {
  PostFields({
    required this.id,
    required this.author,
    required this.title,
    required this.body,
    required this.likes,
    required this.language,
    required this.createdAt,
    required this.pinned,
    required this.tags,
  });

  final int id;
  final int author;
  final String title;
  final String body;
  final int likes;
  final String language;
  final int createdAt;
  final bool pinned;
  final List<String> tags;
}

OrganizationFields organization(int n) => OrganizationFields(
  code(n),
  'organization $n',
  _pick(kinds, n, 1),
  _pick(countries, n, 2),
  1900 + mix(n, 3) % 120,
);

/// The [n]th person, counting from 0, before they have a key. They get
/// `n + 1`.
PersonFields person(int n) => PersonFields(
  id: 0,
  name: 'person $n',
  email: '$n@example.com',
  age: mix(n, 1) % 80,
  country: _pick(countries, n, 2),
  city: 'city ${mix(n, 3) % 500}',
  zip: (mix(n, 4) % 100000).toString().padLeft(5, '0'),
  score: (mix(n, 5) % 1000000) / 1000000,
  active: mix(n, 6) % 7 != 0,
  // A permutation of the people, so that no two join at once.
  joinedAt: 1600000000 + (n * 7907 % peopleCount) * 37,
  organization: code(mix(n, 7) % organizationCount),
  tags: _tagsOf(1 + mix(n, 8) % 3, mix(n, 9)),
);

/// The [m]th post, counting from 0, before it has a key. It gets `m + 1`.
PostFields post(int m) => PostFields(
  id: 0,
  author: 1 + mix(m, 1) % peopleCount,
  title: 'post $m',
  body: [
    for (var at = 0; at < 12; at++) words[mix(m, 100 + at) % 64],
  ].join(' '),
  likes: mix(m, 3) % 1000,
  language: _pick(languages, m, 4),
  // A permutation of the posts, so that no two are made at once.
  createdAt: 1650000000 + (m * 104729 % postCount) * 60,
  pinned: mix(m, 7) % 50 == 0,
  tags: _tagsOf(mix(m, 5) % 3, mix(m, 6)),
);

/// The parameters of each row's runs, counting runs from 0.
abstract final class Runs {
  static int detail(int round) => 1 + mix(round, 11) % peopleCount;

  static (int, String) filter(int round) =>
      (mix(round, 12) % 70, countries[mix(round, 13) % 20]);

  static (String, String) either(int round) =>
      (countries[round % 20], countries[(round + 7) % 20]);

  static String prefix(int round) => 'person ${mix(round, 14) % 1000}';

  static int offset(int round) => round * 997;

  static String authorCountry(int round) => countries[round % 20];

  static (String, String) twoLinks(int round) =>
      (kinds[round % 5], languages[round % 6]);

  static String tag(int round) => tags[round % 20];

  static String phrase(int round) =>
      '${words[mix(round, 15) % 64]} ${words[mix(round, 16) % 64]}';

  static String language(int round) => languages[round % 6];

  /// The countries whose people turn a year older.
  static const updated = ['kr', 'us', 'jp', 'de'];

  /// The people deleted with their posts.
  static int deleted(int round) => 1 + round * 97;

  static const deletedCount = 1000;
}

/// How many people `update-query` changes, which its time is divided by.
int updatedCount() {
  var count = 0;

  for (var n = 0; n < peopleCount; n++) {
    if (Runs.updated.contains(_pick(countries, n, 2))) {
      count++;
    }
  }

  return count;
}

/// The rows of the suite, with how many runs or objects each times.
abstract final class ComplexRows {
  static const load = ('load', organizationCount + peopleCount + postCount);
  static const detail = ('person-detail', 10000);
  static const filter = ('people-filter', 500);
  static const either = ('people-or', 100);
  static const prefix = ('people-prefix', 100);
  static const page = ('people-page', 100);
  static const authorCountry = ('posts-author-country', 100);
  static const twoLinks = ('posts-two-links', 30);
  static const tag = ('posts-tag', 200);
  static const text = ('posts-text', 10);
  static const count = ('posts-count', 200);
  static const update = 'update-query';
  static const delete = 'delete-cascade';
}

/// [items] in lists of [batch].
Iterable<List<T>> batches<T>(List<T> items) sync* {
  for (var at = 0; at < items.length; at += batch) {
    yield items.sublist(
      at,
      at + batch < items.length ? at + batch : items.length,
    );
  }
}
