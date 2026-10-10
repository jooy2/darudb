---
title: Link
order: 3
group: objects
pageClass: reference-page
---

# Link

`Link<T>`는 클래스 `T`의 컬렉션에 있는 객체를 가리키는 링크로, 객체의 기본 키를 가리키는 컬렉션의 타입과 함께 담습니다.

```dart
final class Link<T> {
  const Link(this.key);

  final Object key;
}
```

`Link<User>` 타입의 필드에는 `User`의 기본 키가 들어가고, `List<Link<User>>`는 링크의 목록입니다. `key`는 대상 컬렉션의 키 타입에 맞는 `int`, `String`, `Uint8List`입니다. 다른 타입의 키는 쓸 때 `INVALID_ARGUMENT`로 실패합니다. 없는 객체나 지워진 객체를 가리켜도 되고, 그때는 담긴 키가 그대로 읽힙니다. 두 링크는 키가 같으면 같고, 바이트 키는 내용으로 비교합니다.

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

쿼리는 링크 필드를 `q.author.equals(1)`처럼 키와 비교하거나, `q.author.name`처럼 링크를 거쳐 가리키는 객체의 필드를 읽습니다. 링크 필드에 인덱스를 두면 한 객체를 가리키는 객체를 찾는 일이 조회 한 번으로 끝나며, 백링크는 이렇게 읽습니다.
