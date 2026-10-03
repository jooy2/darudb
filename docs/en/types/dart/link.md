---
title: Link
order: 3
---

# Link

`Link<T>` is a link to an object of the collection of class `T`: the object's primary key, typed by the collection it points into.

```dart
final class Link<T> {
  const Link(this.key);

  final Object key;
}
```

A field of type `Link<User>` holds the primary key of a `User`, and `List<Link<User>>` is a list of links. `key` is an `int`, a `String` or a `Uint8List`, of the linked collection's key type; a key of another type fails to write with `INVALID_ARGUMENT`. A link to an object that does not exist, or no longer does, is allowed, and reads as the key it holds. Two links are equal when their keys are, bytes compared by content.

```dart
@Collection('posts')
class Post {
  const Post({required this.slug, this.author});

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
}

db.write((txn) {
  final alice = txn.collection(userSchema).insert(const User(name: 'Alice'));

  txn.collection(postSchema).insert(Post(slug: 'hello', author: Link<User>(alice)));
});

final byAlice = db.read(
  (txn) => txn.collection(postSchema).find((q) => q.where(q.author.name.equals('Alice'))),
);
```

A query compares a link field with a key, `q.author.equals(1)`, or reads the linked object's fields through it, `q.author.name`. An index on a link field makes finding the objects that link to one object a single lookup, which is how a backlink is read.
