---
title: WriteCollection
order: 8
counterpart: /api/rust/collection-writer
---

# WriteCollection

A `WriteCollection` writes the objects of one collection in a write transaction, and reads them as a `ReadCollection` does.

```ts
interface WriteCollection<O, I> extends ReadCollection<O>
```

`collection` of a write transaction or of a migration returns one. `O` is the type of an object read, and `I` the type of one written, which [InsertOf](../../types/node/object-types.md) makes from the fields: the required fields without a default, and any of the rest. Reads see the transaction's own changes. The methods can be called only while the transaction's function runs; afterwards they throw `CLOSED`.

Every write checks the object against the schema first:

- **`INVALID_ARGUMENT`** for an object that does not fit: a value of another type, a required field left out, a property the schema does not have, a null in a list, an int beyond 2^53 in a field declared with `t.int()`, or a string that UTF-8 cannot hold. A property that is `undefined` or `null` counts as left out.
- **`DUPLICATE_KEY`** when an insert finds its key taken, or a unique index finds one of the object's values taken.
- **A refused write changes nothing**, and the transaction can go on and commit.

A write returns a [Key](../../types/node/key.md): an int key as a number, or as a `bigint` beyond 2^53, a string, or bytes. [Objects](../../guide/objects.md) has the rules the engine keeps.

```ts
db.write((txn) => {
  const users = txn.collection('users');
  const [alice] = users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31 },
    { name: 'Bob', tags: ['new'] }
  ]);

  users.put({ id: 2, name: 'Robert', age: 18 });
  users.update(alice, { age: 32, email: null });
  users.delete(2);
});
```

## Methods

### insert

```ts
insert(object: I): Key;
```

Inserts `object` and returns its primary key. In a collection without a key field, an object without an `id` gets the next number.

### insertMany

```ts
insertMany(objects: readonly I[]): Key[];
```

Inserts `objects` in one call into the engine and returns their keys, in order. A refused object stops the batch with its error, and the objects before it stay inserted in the transaction, which commits them unless the function throws. Anything but an array fails with `INVALID_ARGUMENT`. Encoding a batch into one buffer and crossing into the engine once costs much less than a call for each object.

### put

```ts
put(object: I): Key;
```

Inserts `object`, or replaces the object with its key, and returns the key. It fails as `insert` does, except that a taken key is not a failure. The object replaces the stored one whole, so a field it leaves out holds its default or null.

### putMany

```ts
putMany(objects: readonly I[]): Key[];
```

`put` for each object, in one call into the engine, with the batch rules of `insertMany`.

### update

```ts
update(key: Key, changes: Partial<I>): boolean;
```

Sets the fields that `changes` has in the object whose primary key is `key`, and returns whether there was one; when there is none, nothing is written. The rest of the object stays as it is.

- `null` makes an optional field null, and gives a field with a default its default. A required field without a default cannot be set to null.
- A field that is `undefined` stays as it is.
- An embedded object or a list is replaced whole.

It is refused as `put` is: with `INVALID_ARGUMENT` for a field the collection does not have, a value of another type, or `changes` that is not an object, and with `DUPLICATE_KEY` for a unique value another object holds. Changing the primary key fails with `INVALID_ARGUMENT`; the key the object already has is accepted. Only the changed fields cross into the engine, which changes the record where it lies, so an update costs less than reading the object and putting it back.

### delete

```ts
delete(key: Key): boolean;
```

Deletes the object whose primary key is `key`, with its index entries, and returns whether there was one. A key that is not an int, a string or bytes fails with `INVALID_ARGUMENT`.

## AsyncWriteCollection

```ts
interface AsyncWriteCollection<O, I> extends AsyncReadCollection<O> {
  insert(object: I): Promise<Key>;
  insertMany(objects: readonly I[]): Promise<Key[]>;
  put(object: I): Promise<Key>;
  putMany(objects: readonly I[]): Promise<Key[]>;
  update(key: Key, changes: Partial<I>): Promise<boolean>;
  delete(key: Key): Promise<boolean>;
}
```

The collection of an asynchronous write transaction, with the reads of [AsyncReadCollection](./read-collection.md#asyncreadcollection). Its members do what the members above do and return promises; every failure is a rejection with the same code, and a refused operation changes nothing. Operations run in the order they were called, whether or not each was awaited, and those called together go to the engine as one batch. An operation that fails does not stop the ones called after it.

```ts
await db.writeAsync(async (txn) => {
  const users = txn.collection('users');

  await Promise.all([
    users.insert({ name: 'Dave' }),
    users.update(1, { age: 33 }),
    users.delete(3)
  ]);
});
```
