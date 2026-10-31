//! Deleting objects, and the objects that cascading links take with them
//! (`design/objects.md`, "Cascading links").
//!
//! A delete comes here rather than to the object layer because the links
//! into an object are found the way a query finds objects: through an index
//! that begins with the link field, or by walking the linking collection.
//! The object layer below only takes an object and its entries out.
//!
//! A cascade goes in rounds. The objects deleted last are looked up as one
//! set in each collection that links to them, which is what keeps a large
//! delete by query to one walk of a collection without an index, and the
//! objects found are deleted and make the next round. A key is gathered
//! before anything is deleted, since a walk borrows the transaction a delete
//! needs for itself. Only an object that was there goes on to the next
//! round, so a cycle or a link from an object to itself ends.

use std::ops::Bound;

use super::keys::KeySet;
use super::{Query, found_keys, run};
use crate::error::Result;
use crate::format::object::Value;
use crate::format::object::codec;
use crate::format::object::schema::{Cascade, OpenSchema};
use crate::schema::objects::{self, Source};
use crate::schema::{CollectionType, CollectionWriter, TypedWriter};

impl CollectionWriter<'_> {
    /// Deletes the object whose primary key is `key`, and returns whether
    /// there was one.
    ///
    /// A cascading link ([`Collection::cascade`]) takes the objects that
    /// name the deleted one with it, and those that name them, in this
    /// transaction; the return value says only whether the object `key`
    /// names was there. A failure part-way, of the file or its reading,
    /// leaves the transaction to be aborted, as any such failure does.
    ///
    /// [`Collection::cascade`]: crate::Collection::cascade
    pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool> {
        let key = key.into();
        let found = self.remove(&key)?;

        if found && self.cascades() {
            let (_, _, collection) = self.parts();
            let mut deleted = KeySet::default();

            deleted.push(&objects::key_bytes(collection, &key)?);
            self.cascade(deleted)?;
        }

        Ok(found)
    }

    /// Deletes every object `query` finds, after its sort, offset and limit,
    /// and returns how many there were.
    ///
    /// The keys are gathered first and the objects deleted one after
    /// another, so a query may test the fields it deletes by. A cascading
    /// link takes further objects with them, as [`delete`](Self::delete)
    /// says, which the count leaves out. Nothing can refuse a delete; a
    /// failure part-way, of the file or its reading, leaves the transaction
    /// to be aborted, as any such failure does.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`](crate::Error::InvalidQuery) for a query that
    /// does not fit the schema, and what reading or writing the file meets.
    pub fn delete_where(&mut self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();
        let keys = found_keys(source, schema, collection, query)?;
        let cascades = self.cascades();
        let mut deleted = KeySet::default();

        for key in &keys {
            if self.remove(key)? && cascades {
                let (_, _, collection) = self.parts();

                deleted.push(&objects::key_bytes(collection, key)?);
            }
        }

        if !deleted.is_empty() {
            self.cascade(deleted)?;
        }

        Ok(keys.len() as u64)
    }

    /// Whether a cascading link leads into the collection.
    fn cascades(&self) -> bool {
        let (_, schema, position) = self.place();

        !schema.cascades(position).is_empty()
    }

    /// Deletes what the cascading links into the collection take with the
    /// objects `deleted`, its keys, once they are gone.
    fn cascade(&mut self, deleted: KeySet) -> Result<()> {
        let (_, schema, position) = self.place();
        let collections = schema.schema.collections.len();
        // Each round's objects, by the position of their collection.
        let mut round = vec![(position, deleted.seal())];

        while !round.is_empty() {
            let (source, schema, _) = self.place();
            let mut found: Vec<KeySet> = Vec::new();

            found.resize_with(collections, KeySet::default);

            for (position, keys) in &round {
                for cascade in schema.cascades(*position) {
                    linking(source, schema, *position, cascade, keys, &mut |key| {
                        found[cascade.from].push(key);

                        Ok(())
                    })?;
                }
            }

            round = self.delete_round(found)?;
        }

        Ok(())
    }

    /// Deletes the objects `found` keys, by the position of their collection,
    /// and returns those that were there, in a collection a cascading link
    /// leads into.
    fn delete_round(&mut self, found: Vec<KeySet>) -> Result<Vec<(usize, KeySet)>> {
        let mut next = Vec::new();

        for (position, keys) in found.into_iter().enumerate() {
            if keys.is_empty() {
                continue;
            }

            let keys = keys.seal().in_order();
            let mut gone = KeySet::default();
            let mut writer = self.beside(position);
            let cascades = writer.cascades();

            for at in 0..keys.len() {
                let key = keys.get(at);

                if writer.remove_encoded(key)? && cascades {
                    gone.push(key);
                }
            }

            if !gone.is_empty() {
                next.push((position, gone.seal()));
            }
        }

        Ok(next)
    }
}

/// Gives the primary key of each object of `cascade`'s collection whose link
/// names one of `keys`, keys of the collection at `target`: through its
/// index if it has one, and otherwise by walking the collection once for the
/// whole set.
fn linking(
    source: &dyn Source,
    schema: &OpenSchema,
    target: usize,
    cascade: &Cascade,
    keys: &KeySet,
    give: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let collection = &schema.schema.collections[cascade.from];

    if let Some(index) = cascade.index {
        return run::each_entry_of(source, &collection.indexes[index], keys, give);
    }

    let target = &schema.schema.collections[target];
    let field = collection
        .fields
        .by_id(cascade.field)
        .ok_or_else(|| objects::internal("a cascade through a field its collection lacks"))?;

    source
        .range_in(
            &schema.names(cascade.from).records,
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )?
        .for_each(&mut |key, record| {
            let damaged = |reason: &str| {
                source.corrupted(format!("an object of `{}`: {reason}", collection.name))
            };
            // A record written before the field existed leaves it out, and
            // holds no link.
            let Some(found) = codec::find_field(record, field.id).map_err(damaged)? else {
                return Ok(false);
            };
            let value = codec::field_value(found, &field.kind).map_err(damaged)?;
            let named = objects::with_key_bytes(target, &value, |linked| Ok(keys.contains(linked)))
                .map_err(|_| {
                    source.corrupted(format!(
                        "a link to `{}` holds a key of another type",
                        target.name
                    ))
                })?;

            if named {
                give(key)?;
            }

            Ok(false)
        })
}

impl<T: CollectionType> TypedWriter<'_, T> {
    /// Deletes every object `query` finds, and returns how many there were;
    /// see [`CollectionWriter::delete_where`].
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::delete_where`] fails.
    pub fn delete_where(&mut self, query: &Query) -> Result<u64> {
        self.untyped().delete_where(query)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::format::object::schema::OBJECT_FORMAT;
    use crate::format::object::{Object, Value};
    use crate::schema::objects::check_indexes;
    use crate::schema::{Collection, Migration, Schema, Type};
    use crate::testing::Rng;
    use crate::{Database, Error, OpenOptions, Query, WriteTransaction};

    fn object(fields: &[(&str, Value)]) -> Object {
        let mut object = Object::new();

        for (name, value) in fields {
            object.set(*name, value.clone());
        }

        object
    }

    fn open(dir: &tempfile::TempDir, schema: Schema) -> Database {
        OpenOptions::new()
            .schema(schema)
            .open(dir.path().join("cascades.darudb"))
            .unwrap()
    }

    /// The keys of the objects of `collection`, as ints.
    fn ids(db: &Database, collection: &str) -> Vec<i64> {
        let read = db.begin_read().unwrap();
        let reader = read.collection(collection).unwrap();

        reader
            .iter()
            .unwrap()
            .map(|object| object.unwrap().get("id").unwrap().as_int().unwrap())
            .collect()
    }

    fn check(db: &Database) {
        let read = db.begin_read().unwrap();
        let schema = read.schema().cloned().unwrap();

        check_indexes(&read, &schema.schema).unwrap();
        drop(read);
        assert!(db.check().unwrap().is_ok());
    }

    /// Authors, their books, the reviews of those, and notes that link to a
    /// book without cascading: each collection's link with an index when
    /// `indexed`, and without one otherwise.
    fn library(indexed: bool) -> Schema {
        let with_index = |collection: Collection, field: &str| {
            if indexed {
                collection.index(field)
            } else {
                collection
            }
        };

        Schema::new(1)
            .collection(Collection::new("authors").field("name", Type::String))
            .collection(with_index(
                Collection::new("books")
                    .field("title", Type::String)
                    .field("author", Type::link("authors"))
                    .cascade("author"),
                "author",
            ))
            .collection(with_index(
                Collection::new("reviews")
                    .optional("book", Type::link("books"))
                    .cascade("book"),
                "book",
            ))
            .collection(Collection::new("notes").field("book", Type::link("books")))
    }

    /// One author with `books` books, each with two reviews and a note.
    fn fill(txn: &mut WriteTransaction, books: i64) -> Value {
        let author = txn
            .collection("authors")
            .unwrap()
            .insert(object(&[("name", "A".into())]))
            .unwrap();

        for _ in 0..books {
            let book = txn
                .collection("books")
                .unwrap()
                .insert(object(&[("title", "T".into()), ("author", author.clone())]))
                .unwrap();

            for _ in 0..2 {
                txn.collection("reviews")
                    .unwrap()
                    .insert(object(&[("book", book.clone())]))
                    .unwrap();
            }

            txn.collection("notes")
                .unwrap()
                .insert(object(&[("book", book)]))
                .unwrap();
        }

        author
    }

    #[test]
    fn a_delete_goes_down_the_chain_of_links_and_stops_at_a_plain_link() {
        for indexed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let db = open(&dir, library(indexed));
            let mut txn = db.begin_write().unwrap();
            let first = fill(&mut txn, 2);
            let second = fill(&mut txn, 3);

            // A review of no book, which holds no link to follow.
            txn.collection("reviews")
                .unwrap()
                .insert(Object::new())
                .unwrap();
            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();
            let mut authors = txn.collection("authors").unwrap();

            assert!(authors.delete(first.clone()).unwrap(), "indexed {indexed}");
            assert!(!authors.delete(first).unwrap(), "indexed {indexed}");
            txn.commit().unwrap();

            assert_eq!(ids(&db, "authors"), [second.as_int().unwrap()]);
            assert_eq!(ids(&db, "books"), [3, 4, 5]);
            assert_eq!(ids(&db, "reviews"), [5, 6, 7, 8, 9, 10, 11]);
            // A plain link is left naming a book that is gone.
            assert_eq!(ids(&db, "notes"), [1, 2, 3, 4, 5]);
            check(&db);
        }
    }

    #[test]
    fn an_aborted_delete_takes_nothing_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let db = open(&dir, library(true));
        let mut txn = db.begin_write().unwrap();
        let author = fill(&mut txn, 2);

        txn.commit().unwrap();

        let mut txn = db.begin_write().unwrap();

        txn.collection("authors").unwrap().delete(author).unwrap();
        txn.abort();

        assert_eq!(ids(&db, "books"), [1, 2]);
        assert_eq!(ids(&db, "reviews"), [1, 2, 3, 4]);
        check(&db);
    }

    #[test]
    fn a_cascade_sees_what_the_transaction_wrote() {
        for indexed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let db = open(&dir, library(indexed));
            let mut txn = db.begin_write().unwrap();
            let author = fill(&mut txn, 1);

            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();

            // A new book goes with its author; one moved away stays.
            let added = txn
                .collection("books")
                .unwrap()
                .insert(object(&[
                    ("title", "new".into()),
                    ("author", author.clone()),
                ]))
                .unwrap();
            let moved = txn
                .collection("authors")
                .unwrap()
                .insert(object(&[("name", "B".into())]))
                .unwrap();

            txn.collection("books")
                .unwrap()
                .update(1, object(&[("author", moved)]))
                .unwrap();
            txn.collection("authors").unwrap().delete(author).unwrap();
            txn.commit().unwrap();

            assert_eq!(added.as_int(), Some(2));
            assert_eq!(ids(&db, "books"), [1], "indexed {indexed}");
            check(&db);
        }
    }

    #[test]
    fn cycles_and_links_to_themselves_end() {
        for indexed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let mut nodes = Collection::new("nodes")
                .optional("next", Type::link("nodes"))
                .cascade("next");
            let mut left = Collection::new("left")
                .optional("right", Type::link("right"))
                .cascade("right");
            let mut right = Collection::new("right")
                .optional("left", Type::link("left"))
                .cascade("left");

            if indexed {
                nodes = nodes.index("next");
                left = left.index("right");
                right = right.index("left");
            }

            let db = open(
                &dir,
                Schema::new(1)
                    .collection(nodes)
                    .collection(left)
                    .collection(right),
            );
            let mut txn = db.begin_write().unwrap();
            let mut nodes = txn.collection("nodes").unwrap();

            // 1 -> 2 -> 3 -> 1, 4 -> itself, and 5 -> 6 with nothing after.
            for (id, next) in [(1, 2), (2, 3), (3, 1), (4, 4), (5, 6)] {
                nodes
                    .insert(object(&[("id", id.into()), ("next", next.into())]))
                    .unwrap();
            }

            nodes.insert(object(&[("id", 6.into())])).unwrap();

            let mut sides = txn.collection("left").unwrap();

            sides
                .insert(object(&[("id", 1.into()), ("right", 1.into())]))
                .unwrap();
            sides.insert(object(&[("id", 2.into())])).unwrap();

            let mut sides = txn.collection("right").unwrap();

            sides
                .insert(object(&[("id", 1.into()), ("left", 1.into())]))
                .unwrap();
            sides
                .insert(object(&[("id", 2.into()), ("left", 2.into())]))
                .unwrap();
            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();
            let mut nodes = txn.collection("nodes").unwrap();

            assert!(nodes.delete(2).unwrap());
            assert!(nodes.delete(4).unwrap());
            assert!(nodes.delete(6).unwrap());
            txn.collection("left").unwrap().delete(1).unwrap();
            txn.commit().unwrap();

            assert!(ids(&db, "nodes").is_empty(), "indexed {indexed}");
            assert_eq!(ids(&db, "left"), [2]);
            assert_eq!(ids(&db, "right"), [2]);
            check(&db);
        }
    }

    #[test]
    fn an_object_two_links_reach_is_deleted_once() {
        let dir = tempfile::tempdir().unwrap();
        let schema = Schema::new(1)
            .collection(Collection::new("a"))
            .collection(
                Collection::new("b")
                    .field("a", Type::link("a"))
                    .cascade("a"),
            )
            .collection(
                Collection::new("c")
                    .field("a", Type::link("a"))
                    .field("b", Type::link("b"))
                    .cascade("a")
                    .cascade("b"),
            );
        let db = open(&dir, schema);
        let mut txn = db.begin_write().unwrap();
        let a = txn.collection("a").unwrap().insert(Object::new()).unwrap();
        let b = txn
            .collection("b")
            .unwrap()
            .insert(object(&[("a", a.clone())]))
            .unwrap();

        txn.collection("c")
            .unwrap()
            .insert(object(&[("a", a.clone()), ("b", b)]))
            .unwrap();
        txn.collection("a").unwrap().delete(a).unwrap();
        txn.commit().unwrap();

        assert!(ids(&db, "b").is_empty());
        assert!(ids(&db, "c").is_empty());
        check(&db);
    }

    #[test]
    fn a_delete_by_query_counts_what_it_found_and_takes_the_rest() {
        for indexed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let db = open(&dir, library(indexed));
            let mut txn = db.begin_write().unwrap();
            let authors: Vec<Value> = (0..400).map(|_| fill(&mut txn, 3)).collect();

            txn.commit().unwrap();

            let mut txn = db.begin_write().unwrap();
            let query = Query::new().filter(crate::Filter::lt("id", 101));
            let deleted = txn
                .collection("authors")
                .unwrap()
                .delete_where(&query)
                .unwrap();

            txn.commit().unwrap();

            assert_eq!(deleted, 100, "the count leaves out what cascades");
            assert_eq!(ids(&db, "authors").len(), authors.len() - 100);
            assert_eq!(ids(&db, "books").len(), 300 * 3);
            assert_eq!(ids(&db, "reviews").len(), 300 * 3 * 2);
            assert_eq!(ids(&db, "notes").len(), 400 * 3);
            check(&db);
        }
    }

    #[test]
    fn a_link_that_names_nothing_is_nothing_to_follow() {
        for indexed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let db = open(&dir, library(indexed));
            let mut txn = db.begin_write().unwrap();

            txn.collection("books")
                .unwrap()
                .insert(object(&[("title", "T".into()), ("author", 7.into())]))
                .unwrap();
            assert!(!txn.collection("authors").unwrap().delete(7).unwrap());
            txn.commit().unwrap();

            assert_eq!(ids(&db, "books"), [1], "indexed {indexed}");

            // Nor does an object deleted later find what its key once named.
            let mut txn = db.begin_write().unwrap();
            let mut authors = txn.collection("authors").unwrap();

            authors
                .insert(object(&[("id", 8.into()), ("name", "A".into())]))
                .unwrap();
            assert!(authors.delete(8).unwrap());
            txn.commit().unwrap();

            assert_eq!(ids(&db, "books"), [1], "indexed {indexed}");
            check(&db);
        }
    }

    /// A key of any type, through each kind of index a link may begin, in a
    /// short index, which a cascade walks whole, and a long one, which it
    /// looks each key up in. The key deleted begins the key kept, which an
    /// entry's bounds have to tell apart.
    #[test]
    fn a_cascade_reads_every_index_that_begins_with_its_link() {
        const NAMES: [&str; 6] = ["plain", "first", "second", "unique", "both", "none"];

        let linking = |name: &str| {
            Collection::new(name)
                .optional("day", Type::Int)
                .optional("owner", Type::link("owners"))
                .cascade("owner")
        };
        let schema = |key: Type| {
            Schema::new(1)
                .collection(Collection::new("owners").primary_key("key", key))
                .collection(linking("plain").index("owner"))
                .collection(linking("first").index_on(["owner", "day"]))
                .collection(linking("second").index_on(["day", "owner"]))
                .collection(linking("unique").unique("owner"))
                .collection(linking("both").unique_on(["owner", "day"]))
                .collection(linking("none"))
        };

        for (kind, gone, keep) in [
            (Type::Int, Value::Int(1), Value::Int(2)),
            (Type::String, Value::from("a"), Value::from("ab")),
            (Type::Bytes, Value::Bytes(vec![0]), Value::Bytes(vec![0, 0])),
        ] {
            for others in [0, 80] {
                let dir = tempfile::tempdir().unwrap();
                let db = open(&dir, schema(kind.clone()));
                let mut txn = db.begin_write().unwrap();

                for owner in [&keep, &gone] {
                    txn.collection("owners")
                        .unwrap()
                        .insert(object(&[("key", owner.clone())]))
                        .unwrap();
                }

                for name in NAMES {
                    let mut objects = txn.collection(name).unwrap();

                    objects
                        .insert(object(&[("owner", keep.clone()), ("day", 1.into())]))
                        .unwrap();
                    objects
                        .insert(object(&[("owner", gone.clone()), ("day", 2.into())]))
                        .unwrap();
                    // No owner, and no day, which a unique index keys with
                    // the object's own key.
                    objects.insert(object(&[("day", 3.into())])).unwrap();

                    if name != "unique" {
                        objects.insert(object(&[("owner", gone.clone())])).unwrap();
                    }

                    for day in 0..i64::try_from(others).unwrap() {
                        objects
                            .insert(if name == "unique" {
                                object(&[("day", day.into())])
                            } else {
                                object(&[("owner", keep.clone()), ("day", (10 + day).into())])
                            })
                            .unwrap();
                    }
                }

                txn.commit().unwrap();

                let mut txn = db.begin_write().unwrap();

                assert!(
                    txn.collection("owners")
                        .unwrap()
                        .delete(gone.clone())
                        .unwrap()
                );
                txn.commit().unwrap();

                for name in NAMES {
                    let left = ids(&db, name);

                    assert_eq!(left[..2], [1, 3], "{name}");
                    assert_eq!(left.len(), others + 2, "{name}");
                }

                check(&db);
            }
        }
    }

    #[test]
    fn a_handle_without_a_schema_and_a_schema_read_back_keep_the_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cascades.darudb");
        let db = open(&dir, library(true));
        let mut txn = db.begin_write().unwrap();
        let author = fill(&mut txn, 1);

        txn.commit().unwrap();
        drop(db);

        let db = Database::open(&path).unwrap();
        let stored = db.stored_schema().unwrap();
        let books = &stored.collections()[1];
        let author_field = books.fields().iter().find(|f| f.name() == "author");

        assert!(author_field.unwrap().is_cascade());
        assert!(!books.fields()[0].is_cascade());
        assert_eq!(stored, library(true));

        let mut txn = db.begin_write().unwrap();

        txn.collection("authors").unwrap().delete(author).unwrap();
        txn.commit().unwrap();

        assert!(ids(&db, "reviews").is_empty());
        drop(db);

        // The same version with another cascade is another schema.
        let mut options = OpenOptions::new();
        let mut changed = library(true);

        changed.collections[2] = Collection::new("reviews")
            .optional("book", Type::link("books"))
            .index("book");
        options.schema(changed);

        assert_eq!(
            options.open(&path).err().map(|error| error.code()),
            Some("SCHEMA_MISMATCH")
        );
    }

    #[test]
    fn a_cascade_is_only_on_a_link_to_one_object_of_the_collection() {
        let refused = |collection: Collection| {
            let schema = Schema::new(1)
                .collection(Collection::new("a"))
                .collection(collection);

            assert_eq!(
                schema.validate().err().map(|error| error.code()),
                Some("INVALID_ARGUMENT"),
                "{schema:?}"
            );
        };
        let linking = || {
            Collection::new("b")
                .field("name", Type::String)
                .field("links", Type::list(Type::link("a")))
                .optional("link", Type::link("a"))
        };

        refused(linking().cascade("missing"));
        refused(linking().cascade("name"));
        refused(linking().cascade("links"));
        refused(linking().cascade("id"));
        // A field declared after the call is not there yet.
        refused(
            Collection::new("b")
                .cascade("link")
                .field("link", Type::link("a")),
        );
        assert!(
            Schema::new(1)
                .collection(Collection::new("a"))
                .collection(linking().cascade("link"))
                .validate()
                .is_ok()
        );
    }

    fn versions(version: u64, cascade: bool) -> Schema {
        let posts = Collection::new("posts").optional("author", Type::link("users"));

        Schema::new(version)
            .collection(Collection::new("users"))
            .collection(if cascade {
                posts.cascade("author")
            } else {
                posts
            })
    }

    fn object_format(db: &Database) -> i64 {
        crate::format::object::schema::StoredSchema::decode(db.schema_record().unwrap())
            .unwrap()
            .format
    }

    /// A cascade added by a migration raises the object format, writes no
    /// record, and works in the migration's own function under the schema
    /// the migration moves to. Taken away again, it leaves the format.
    #[test]
    fn a_migration_adds_a_cascade_and_deletes_through_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cascades.darudb");
        let db = open(&dir, versions(1, false));
        let mut txn = db.begin_write().unwrap();

        for _ in 0..3 {
            txn.collection("users")
                .unwrap()
                .insert(Object::new())
                .unwrap();
        }

        for author in [1, 2, 3] {
            txn.collection("posts")
                .unwrap()
                .insert(object(&[("author", author.into())]))
                .unwrap();
        }

        txn.commit().unwrap();
        assert_eq!(object_format(&db), 1);
        drop(db);

        let mut kept = OpenOptions::new();

        kept.schema(versions(2, true)).upgrade_format(false);

        let refused = kept.open(&path).err().unwrap();

        assert_eq!(refused.code(), "INVALID_ARGUMENT");
        assert!(refused.to_string().contains("upgrade_format"), "{refused}");

        let db = OpenOptions::new()
            .schema(versions(2, true))
            .migration(Migration::to(2).run(|migrating| {
                migrating.collection("users")?.delete(1)?;

                Ok(())
            }))
            .open(&path)
            .unwrap();

        assert_eq!(object_format(&db), OBJECT_FORMAT);
        assert_eq!(ids(&db, "users"), [2, 3]);
        assert_eq!(ids(&db, "posts"), [2, 3]);

        let mut txn = db.begin_write().unwrap();

        txn.collection("users").unwrap().delete(2).unwrap();
        txn.commit().unwrap();
        assert_eq!(ids(&db, "posts"), [3]);
        drop(db);

        let db = OpenOptions::new()
            .schema(versions(3, false))
            .open(&path)
            .unwrap();
        let mut txn = db.begin_write().unwrap();

        txn.collection("users").unwrap().delete(3).unwrap();
        txn.commit().unwrap();

        assert_eq!(object_format(&db), OBJECT_FORMAT);
        assert_eq!(ids(&db, "posts"), [3], "the link no longer cascades");
        check(&db);
    }

    /// Objects of three collections with links among them, the cycles
    /// through `a` included, deleted at random: what is left is what
    /// following the links on a model leaves, with an index or without.
    #[test]
    fn random_deletes_leave_what_the_links_say() {
        type Model = [BTreeMap<i64, Vec<(usize, i64)>>; 3];

        const NAMES: [&str; 3] = ["a", "b", "c"];
        // Each collection's cascading links: the field and the collection it
        // names.
        const LINKS: [&[(&str, usize)]; 3] =
            [&[("to_c", 2), ("to_a", 0)], &[("to_a", 0)], &[("to_b", 1)]];

        fn schema(indexed: bool) -> Schema {
            let mut schema = Schema::new(1);

            for (name, links) in NAMES.iter().zip(LINKS) {
                let mut collection = Collection::new(*name);

                for (field, target) in links {
                    collection = collection
                        .optional(*field, Type::link(NAMES[*target]))
                        .cascade(*field);

                    if indexed {
                        collection = collection.index(*field);
                    }
                }

                schema = schema.collection(collection);
            }

            schema
        }

        fn closure(model: &mut Model, start: Vec<(usize, i64)>) {
            let mut work = start;

            while let Some((collection, key)) = work.pop() {
                if model[collection].remove(&key).is_none() {
                    continue;
                }

                for (from, objects) in model.iter().enumerate() {
                    for (object, links) in objects {
                        if links.contains(&(collection, key)) {
                            work.push((from, *object));
                        }
                    }
                }
            }
        }

        for seed in 0..12 {
            let mut rng = Rng::new(seed);
            let dir = tempfile::tempdir().unwrap();
            let db = open(&dir, schema(seed % 2 == 0));
            let mut model: Model = Default::default();
            let mut txn = db.begin_write().unwrap();

            for (position, links) in LINKS.iter().enumerate() {
                let mut objects = txn.collection(NAMES[position]).unwrap();

                for key in 1..=40 {
                    let mut object = object(&[("id", key.into())]);
                    let mut held = Vec::new();

                    for (field, target) in *links {
                        if rng.below(3) > 0 {
                            let linked = 1 + i64::try_from(rng.below(45)).unwrap();

                            object.set(*field, linked);
                            held.push((*target, linked));
                        }
                    }

                    objects.insert(object).unwrap();
                    model[position].insert(key, held);
                }
            }

            txn.commit().unwrap();

            for _ in 0..30 {
                let position = rng.index(3);
                let key = 1 + i64::try_from(rng.below(45)).unwrap();
                let mut txn = db.begin_write().unwrap();
                let mut objects = txn.collection(NAMES[position]).unwrap();

                if rng.below(3) == 0 {
                    let found: Vec<i64> = model[position]
                        .keys()
                        .copied()
                        .filter(|existing| *existing >= key)
                        .collect();
                    let query = Query::new().filter(crate::Filter::ge("id", key));

                    assert_eq!(objects.delete_where(&query).unwrap(), found.len() as u64);
                    closure(
                        &mut model,
                        found.iter().map(|key| (position, *key)).collect(),
                    );
                } else {
                    let existed = model[position].contains_key(&key);

                    assert_eq!(objects.delete(key).unwrap(), existed);
                    closure(&mut model, vec![(position, key)]);
                }

                txn.commit().unwrap();

                for (position, name) in NAMES.iter().enumerate() {
                    let expected: Vec<i64> = model[position].keys().copied().collect();

                    assert_eq!(ids(&db, name), expected, "seed {seed}");
                }
            }

            check(&db);
        }
    }

    #[test]
    fn a_cascade_finds_a_link_of_another_type_damaged() {
        let dir = tempfile::tempdir().unwrap();
        let db = open(&dir, library(false));
        let mut txn = db.begin_write().unwrap();
        let author = fill(&mut txn, 1);
        let schema = txn.schema().cloned().unwrap();
        let books = schema.schema.collection("books").unwrap();
        let author_field = books.fields.by_name("author").unwrap().id;
        let record = crate::format::object::codec::write(&[(
            author_field,
            crate::format::object::codec::Raw::Link(Box::new(
                crate::format::object::codec::Raw::String("not an int".to_owned()),
            )),
        )]);

        txn.insert_in(
            &crate::format::object::names::records(books.id),
            &crate::format::object::key::encoded(&Value::Int(9)).unwrap(),
            &record,
        )
        .unwrap();

        let error = txn
            .collection("authors")
            .unwrap()
            .delete(author)
            .err()
            .unwrap();

        assert!(matches!(error, Error::Corrupted { .. }), "{error}");
    }
}
