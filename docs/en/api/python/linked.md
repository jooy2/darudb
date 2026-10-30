---
title: linked
order: 13
---

# linked

`linked` makes a condition on the objects of another collection, or of the same one, that link to an object.

```python
def linked(
    linking: type | str, link: str | FieldRef, condition: Condition | None = None
) -> Condition: ...
```

The condition holds for an object that some object of `linking`, a collection's class or its name, links to through its link field `link`, and that meets `condition`, on `linking`'s attributes, if it is given: one linking object has to meet it whole. `~` of it finds the objects nothing links to. The link is an attribute's name, a path such as `"meta.author"`, or an `F` path, and has to link to the collection the query runs on; a to-one link or a list of links. The package gives the engine the names the file stores, through `linking`'s class, and names pass as they are for a collection given by its name.

A collection the schema does not have, or a field that is not such a link, fails with `INVALID_QUERY` when the query runs; `linking` that is neither a class nor a name, or an empty link, fails at once. It is `ANY posts.author (year >= 2020)` in the [query language](../../guide/query-language.md#objects-that-link-here).

```python
from darudb import F, linked

with db.read() as txn:
    users = txn.collection(User)

    users.find(linked(Post, "author", F.year >= 2020))
    users.find(~linked(Post, "author"))
    users.find(linked(Post, "readers") & (F.age > 40))
```
