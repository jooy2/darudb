//! Tests of the object layer that look below the public API: the index trees
//! against the objects they index.

use std::ops::Bound;

use super::objects::{Source, check_indexes};
use super::typed::{CollectionType, FieldReader, ValueWriter};
use super::{Collection, Schema, Type};
use crate::format::object::codec::Raw;
use crate::format::object::names::{index_tree, records};
use crate::format::object::schema::Kind;
use crate::format::object::{Object, Value};
use crate::testing::Rng;
use crate::{Database, OpenOptions};

fn schema() -> Schema {
    Schema::new(1)
        .collection(Collection::new("teams").primary_key("name", Type::String))
        .collection(
            Collection::new("players")
                .with_default("score", Type::Int, 0)
                .optional("handle", Type::String)
                .optional("tags", Type::list(Type::String))
                .optional("team", Type::link("teams"))
                .optional("friends", Type::list(Type::link("players")))
                .index("score")
                .unique("handle")
                .index("tags")
                .index("team")
                .index("friends"),
        )
}

/// A player with a few fields left out at random, and values drawn from
/// small sets so that they repeat.
fn player(rng: &mut Rng) -> Object {
    let mut object = Object::new();
    let pick = |rng: &mut Rng, of: &[&str]| of[rng.index(of.len())].to_owned();

    if rng.below(4) > 0 {
        object.set("id", 1 + i64::try_from(rng.below(40)).unwrap());
    }

    if rng.below(3) > 0 {
        object.set("score", i64::try_from(rng.below(5)).unwrap() - 2);
    }

    if rng.below(2) > 0 {
        object.set(
            "handle",
            pick(rng, &["ace", "bee", "cat", "dot", "eel", "a\0b", ""]),
        );
    }

    if rng.below(2) > 0 {
        let tags = (0..rng.below(4))
            .map(|_| Value::from(pick(rng, &["red", "blue", "green", "red"])))
            .collect::<Vec<_>>();

        object.set("tags", tags);
    }

    if rng.below(2) > 0 {
        object.set("team", pick(rng, &["north", "south"]));
    }

    if rng.below(3) == 0 {
        let friends = (0..rng.below(3))
            .map(|_| Value::from(1 + i64::try_from(rng.below(40)).unwrap()))
            .collect::<Vec<_>>();

        object.set("friends", friends);
    }

    object
}

#[test]
fn random_writes_keep_every_index_in_step_with_the_objects() {
    for seed in 0..8 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects.darudb");
        let mut options = OpenOptions::new();

        if seed % 2 == 1 {
            options.key([7; 32]);
        }

        options.schema(schema());

        let db = options.open(&path).unwrap();
        let mut rng = Rng::new(seed);
        let mut refused = 0;
        let mut updates = 0;

        for round in 0..30 {
            let mut txn = db.begin_write().unwrap();
            let open = txn.schema().cloned().unwrap();

            for _ in 0..40 {
                let mut players = txn.collection("players").unwrap();
                let result = match rng.below(12) {
                    0..=4 => players.insert(player(&mut rng)).map(drop),
                    5..=7 => players.put(player(&mut rng)).map(drop),
                    8..=9 => players
                        .delete(1 + i64::try_from(rng.below(40)).unwrap())
                        .map(drop),
                    _ => {
                        let key = 1 + i64::try_from(rng.below(40)).unwrap();
                        let mut changes = player(&mut rng);

                        changes.remove("id");

                        for field in ["score", "handle", "tags", "team", "friends"] {
                            if rng.below(6) == 0 {
                                changes.set(field, Value::Null);
                            }
                        }

                        let before = players.get(key).unwrap();
                        let updated = players.update(key, changes.clone());
                        let after = players.get(key).unwrap();

                        // What the object is after an update is what it was
                        // with the changes set, as a put would write it, and
                        // a refused update leaves it as it was.
                        match (&updated, before) {
                            (Ok(true), Some(mut expected)) => {
                                for (field, value) in changes.fields() {
                                    let value = match (field, value) {
                                        ("score", Value::Null) => Value::Int(0),
                                        (_, value) => value.clone(),
                                    };

                                    expected.set(field, value);
                                }

                                assert_eq!(after, Some(expected), "seed {seed}");
                                updates += 1;
                            }
                            (Ok(false), None) => assert_eq!(after, None, "seed {seed}"),
                            (Err(_), before) => assert_eq!(after, before, "seed {seed}"),
                            (updated, before) => {
                                panic!("seed {seed}: {updated:?} for {before:?}")
                            }
                        }

                        updated.map(drop)
                    }
                };

                match result {
                    Ok(()) => {}
                    Err(error) if error.code() == "DUPLICATE_KEY" => refused += 1,
                    Err(error) => panic!("seed {seed}: {error}"),
                }
            }

            check_indexes(&txn as &dyn Source, &open.schema)
                .unwrap_or_else(|reason| panic!("seed {seed}, round {round}: {reason}"));

            if rng.below(4) == 0 {
                txn.abort();
            } else {
                txn.commit().unwrap();
            }
        }

        drop(db);

        let db = options.open(&path).unwrap();
        let read = db.begin_read().unwrap();

        check_indexes(&read as &dyn Source, &read.schema().unwrap().schema)
            .unwrap_or_else(|reason| panic!("seed {seed}, reopened: {reason}"));
        assert!(
            refused > 0,
            "seed {seed}: the unique index never refused a value"
        );
        assert!(read.collection("players").unwrap().len().unwrap() > 0);
        assert!(
            updates > 20,
            "seed {seed}: only {updates} updates changed an object"
        );
    }
}

#[test]
fn an_index_built_by_a_migration_matches_one_kept_all_along() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let bare = Schema::new(1)
        .collection(Collection::new("teams").primary_key("name", Type::String))
        .collection(
            Collection::new("players")
                .with_default("score", Type::Int, 0)
                .optional("handle", Type::String)
                .optional("tags", Type::list(Type::String))
                .optional("team", Type::link("teams"))
                .optional("friends", Type::list(Type::link("players"))),
        );
    let db = OpenOptions::new().schema(bare).open(&path).unwrap();
    let mut rng = Rng::new(99);
    let mut txn = db.begin_write().unwrap();
    let mut handles = std::collections::BTreeSet::new();

    // More objects than one batch of the build, with unique handles.
    for _ in 0..2500 {
        let mut object = player(&mut rng);

        object.remove("id");

        if let Some(handle) = object.remove("handle") {
            if handles.insert(handle.as_str().unwrap().to_owned()) {
                object.set("handle", handle);
            }
        }

        txn.collection("players").unwrap().insert(object).unwrap();
    }

    txn.commit().unwrap();
    drop(db);

    let mut v2 = schema();

    v2.version = 2;

    let db = OpenOptions::new().schema(v2).open(&path).unwrap();
    let read = db.begin_read().unwrap();

    check_indexes(&read as &dyn Source, &read.schema().unwrap().schema).unwrap();
    assert_eq!(
        read.len_in("\0idx/1").unwrap(),
        2500,
        "every player has a score"
    );
}

#[test]
fn a_changed_default_rebuilds_the_indexes_of_its_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let v1 = Schema::new(1).collection(Collection::new("items").field("name", Type::String));
    let db = OpenOptions::new().schema(v1).open(&path).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("items")
        .unwrap()
        .insert(Object::new().with("name", "old"))
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    // The item was written before `rank` existed, so it reads the default,
    // whichever it is at the time.
    for (version, default) in [(2, 1), (3, 5)] {
        let schema = Schema::new(version).collection(
            Collection::new("items")
                .field("name", Type::String)
                .with_default("rank", Type::Int, default)
                .index("rank"),
        );
        let db = OpenOptions::new().schema(schema).open(&path).unwrap();
        let read = db.begin_read().unwrap();

        check_indexes(&read as &dyn Source, &read.schema().unwrap().schema).unwrap();
        assert_eq!(
            read.collection("items")
                .unwrap()
                .get(1)
                .unwrap()
                .unwrap()
                .get("rank"),
            Some(&Value::Int(default))
        );
    }
}

#[test]
fn a_handle_opened_without_a_schema_can_open_one_later() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let plain = Database::open(&path).unwrap();
    let typed = OpenOptions::new().schema(schema()).open(&path).unwrap();

    assert!(plain.begin_read().unwrap().schema().is_none());
    assert_eq!(
        plain
            .begin_read()
            .unwrap()
            .collection("players")
            .err()
            .map(|error| error.code()),
        Some("INVALID_ARGUMENT")
    );
    assert!(typed.begin_read().unwrap().collection("players").is_ok());
}

/// The schema of [`schema`] at version 2, with a field more.
fn schema_v2() -> Schema {
    let mut schema = schema();

    schema.version = 2;
    schema.collections[1] = schema.collections[1]
        .clone()
        .optional("nickname", Type::String);
    schema
}

/// Run by [`a_process_whose_file_another_process_migrated_fails_with_schema_mismatch`]:
/// migrates the database to [`schema_v2`] and says so.
#[test]
fn helper_migrating_to_version_2() {
    let Ok(path) = std::env::var(crate::testing::HELPER_PATH) else {
        return;
    };
    let db = OpenOptions::new().schema(schema_v2()).open(path).unwrap();

    println!("answer migrated");
    crate::testing::wait_to_be_told();
    drop(db);
}

#[test]
fn a_process_whose_file_another_process_migrated_fails_with_schema_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let db = OpenOptions::new().schema(schema()).open(&path).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("players")
        .unwrap()
        .insert(Object::new().with("handle", "ace"))
        .unwrap();
    txn.commit().unwrap();

    let before = db.begin_read().unwrap();
    let mut other =
        crate::testing::Helper::spawn("schema::tests::helper_migrating_to_version_2", &path);

    assert_eq!(other.answer(), "migrated");

    let code = |result: crate::Result<_>| result.err().map(|error: crate::Error| error.code());

    assert_eq!(
        code(db.begin_read().unwrap().collection("players").map(drop)),
        Some("SCHEMA_MISMATCH")
    );
    assert_eq!(
        code(db.begin_write().unwrap().collection("players").map(drop)),
        Some("SCHEMA_MISMATCH")
    );
    assert_eq!(
        before.collection("players").unwrap().len().unwrap(),
        1,
        "a snapshot from before the migration reads under the old schema"
    );

    other.tell();
    assert!(other.finish().0);

    let again = OpenOptions::new().schema(schema_v2()).open(&path).unwrap();
    let read = again.begin_read().unwrap();

    assert_eq!(
        read.collection("players")
            .unwrap()
            .get(1)
            .unwrap()
            .unwrap()
            .get("nickname"),
        Some(&Value::Null)
    );
}

/// Two collections whose fields all hold scalars, one keyed by an
/// auto-increment and one by a string, so that a binding's records for them
/// take the path that checks and completes the record as it is.
fn flat_schema() -> Schema {
    Schema::new(1)
        .collection(flat_people())
        .collection(flat_tags())
}

fn flat_people() -> Collection {
    Collection::new("people")
        .field("name", Type::String)
        .optional("email", Type::String)
        .with_default("age", Type::Int, 18)
        .optional("score", Type::Float)
        .with_default("admin", Type::Bool, false)
        .optional("photo", Type::Bytes)
        .unique("email")
        .index("age")
        .index("admin")
}

fn flat_tags() -> Collection {
    Collection::new("tags")
        .primary_key("label", Type::String)
        .with_default("uses", Type::Int, 0)
        .index("uses")
}

/// A record as a binding might send one for a collection whose fields are
/// `fields`, by id and kind: most fields at random, now and then a value of
/// the wrong type, an id the collection does not have, the key, or bytes
/// cut or changed.
fn random_record(rng: &mut Rng, fields: &[(u64, &str)]) -> Vec<u8> {
    use crate::format::object::codec;

    let mut raw = Vec::new();

    for (id, kind) in fields {
        if rng.below(3) == 0 {
            continue;
        }

        let kind = if rng.below(30) == 0 {
            ["string", "int", "float", "bool", "bytes"][rng.index(5)]
        } else {
            kind
        };
        let value = match kind {
            "string" => {
                Raw::String(["ace", "bee", "cat", "", "a\0b", "é"][rng.index(6)].to_owned())
            }
            "int" => Raw::Int(i64::try_from(rng.below(8)).unwrap() - 2),
            "float" => Raw::Float([0.5, -1.0, f64::NAN, 1e300][rng.index(4)]),
            "bool" => Raw::Bool(rng.below(2) == 0),
            _ => {
                let len = rng.index(4);

                Raw::Bytes(rng.bytes(len))
            }
        };

        raw.push((*id, value));
    }

    if rng.below(30) == 0 {
        raw.push((90 + rng.below(3), Raw::Int(1)));
    }

    let mut record = codec::write(&raw);

    match rng.below(40) {
        0 => record.truncate(rng.index(record.len())),
        1 => {
            let at = rng.index(record.len());

            record[at] = u8::try_from(rng.below(256)).unwrap();
        }
        _ => {}
    }

    record
}

#[test]
fn a_binding_s_records_are_written_as_the_objects_they_hold_would_be() {
    use crate::format::object::codec;

    for seed in 0..6 {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(flat_schema());

        // Records through the path that completes them as they are, and the
        // same records read into objects and written as objects.
        let through_records = options.open(dir.path().join("records.darudb")).unwrap();
        let through_objects = options.open(dir.path().join("objects.darudb")).unwrap();
        let mut rng = Rng::new(seed);
        let mut written = 0;

        for _ in 0..20 {
            let mut left = through_records.begin_write().unwrap();
            let mut right = through_objects.begin_write().unwrap();
            let open = left.schema().cloned().unwrap();

            for _ in 0..30 {
                let position = rng.index(2);
                let definition = &open.schema.collections[position];
                let name = definition.name.clone();
                let kinds: Vec<(u64, &str)> = definition
                    .fields
                    .list
                    .iter()
                    .map(|field| {
                        let kind = match field.kind {
                            Kind::String => "string",
                            Kind::Int => "int",
                            Kind::Float => "float",
                            Kind::Bool => "bool",
                            _ => "bytes",
                        };

                        (field.id, kind)
                    })
                    .collect();
                let record = random_record(&mut rng, &kinds);
                let replace = rng.below(2) == 0;
                let fast = {
                    let mut collection = left.collection(&name).unwrap();

                    if replace {
                        collection.put_record(&record)
                    } else {
                        collection.insert_record(&record)
                    }
                };
                let slow = match codec::partial_object_of(&record, &definition.fields) {
                    Ok(object) => {
                        let mut collection = right.collection(&name).unwrap();

                        if replace {
                            collection.put(object)
                        } else {
                            collection.insert(object)
                        }
                    }
                    Err(reason) => Err(crate::Error::InvalidArgument {
                        message: format!("a record for `{name}`: {reason}"),
                    }),
                };

                match (&fast, &slow) {
                    (Ok(fast), Ok(slow)) => {
                        assert_eq!(fast, slow, "seed {seed}");
                        written += 1;
                    }
                    (Err(fast), Err(slow)) => {
                        assert_eq!(fast.to_string(), slow.to_string(), "seed {seed}");
                    }
                    _ => panic!(
                        "seed {seed}: {record:?} gave {fast:?} one way and {slow:?} the other"
                    ),
                }
            }

            // The same records, byte for byte, and the same index entries.
            for collection in &open.schema.collections {
                let trees = std::iter::once(records(collection.id))
                    .chain(collection.indexes.iter().map(|index| index_tree(index.id)));

                for tree in trees {
                    let entries = |txn: &crate::WriteTransaction| {
                        txn.range_in::<Vec<u8>>(
                            &tree,
                            &(Bound::<Vec<u8>>::Unbounded, Bound::<Vec<u8>>::Unbounded),
                            false,
                        )
                        .unwrap()
                        .collect::<crate::Result<Vec<_>>>()
                        .unwrap()
                    };

                    assert_eq!(
                        entries(&left),
                        entries(&right),
                        "seed {seed}: tree {:?}",
                        &*tree
                    );
                }
            }

            check_indexes(&left as &dyn Source, &open.schema)
                .unwrap_or_else(|reason| panic!("seed {seed}: {reason}"));
            left.commit().unwrap();
            right.commit().unwrap();
        }

        assert!(written > 100, "seed {seed}: only {written} records written");
    }
}

/// An object of `people` in [`flat_schema`] as a type written by hand
/// might write it: each field's value by the field's slot, `None` for null,
/// of whatever kind it holds, the field's or another.
struct FlatPerson(Vec<Option<Raw>>);

/// An object of `tags`, as [`FlatPerson`] is one of `people`.
struct FlatTag(Vec<Option<Raw>>);

/// Writes `value` as a type written by hand writes a field's value.
fn write_raw(value: Option<&Raw>, writer: ValueWriter<'_>) -> crate::Result<()> {
    match value {
        None => writer.null(),
        Some(Raw::Bool(value)) => writer.bool(*value),
        Some(Raw::Int(value)) => writer.int(*value),
        Some(Raw::Float(value)) => writer.float(*value),
        Some(Raw::String(value)) => writer.string(value),
        Some(Raw::Bytes(value)) => writer.bytes(value),
        Some(other) => panic!("not a scalar: {other:?}"),
    }
}

fn not_read() -> crate::Error {
    crate::Error::Internal {
        message: "the test reads no typed object".to_owned(),
    }
}

impl CollectionType for FlatPerson {
    type Key = i64;

    const COLLECTION: &'static str = "people";

    fn collection() -> Collection {
        flat_people()
    }

    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> crate::Result<()> {
        write_raw(self.0[slot].as_ref(), value)
    }

    fn read(_: FieldReader<'_>) -> crate::Result<Self> {
        Err(not_read())
    }
}

impl CollectionType for FlatTag {
    type Key = String;

    const COLLECTION: &'static str = "tags";

    fn collection() -> Collection {
        flat_tags()
    }

    fn write_field(&self, slot: usize, value: ValueWriter<'_>) -> crate::Result<()> {
        write_raw(self.0[slot].as_ref(), value)
    }

    fn read(_: FieldReader<'_>) -> crate::Result<Self> {
        Err(not_read())
    }
}

/// The values of an object of the collection whose fields are `kinds`, by
/// slot, the key first: most of them of their own kind and drawn from small
/// sets, so that keys and unique values repeat; now and then null, or a value
/// of another kind. `auto` says whether the key is an auto-increment, which
/// an object mostly leaves out.
fn random_values(rng: &mut Rng, kinds: &[&str], auto: bool) -> Vec<Option<Raw>> {
    kinds
        .iter()
        .enumerate()
        .map(|(slot, kind)| {
            let leave_out = match (slot, auto) {
                (0, true) => rng.below(4) > 0,
                (0, false) => rng.below(30) == 0,
                _ => rng.below(4) == 0,
            };

            if leave_out {
                return None;
            }

            let kind = if rng.below(30) == 0 {
                ["string", "int", "float", "bool", "bytes"][rng.index(5)]
            } else {
                kind
            };

            Some(match kind {
                "string" => {
                    Raw::String(["ace", "bee", "cat", "dot", "é\0"][rng.index(5)].to_owned())
                }
                "int" => Raw::Int(i64::try_from(rng.below(40)).unwrap() - 2),
                "float" => Raw::Float([0.5, -1.0, 1e300][rng.index(3)]),
                "bool" => Raw::Bool(rng.below(2) == 0),
                _ => {
                    let len = rng.index(4);

                    Raw::Bytes(rng.bytes(len))
                }
            })
        })
        .collect()
}

/// A typed write stores what a binding's record of the same object stores,
/// and fails where that fails, with the same error: the same records, byte
/// for byte, the same index entries and the same auto-increment counters.
/// The types are written by hand, so that now and then they leave out a
/// field the collection requires, with a default or without, or write a
/// value of another kind, which the record a typed writer encodes is not
/// checked for as a binding's is.
#[test]
fn a_typed_write_stores_what_a_binding_s_record_of_the_object_stores() {
    use crate::format::object::codec;
    use crate::format::object::names::{META, counter};

    for seed in 0..6 {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(flat_schema());

        let typed = options.open(dir.path().join("typed.darudb")).unwrap();
        let through_records = options.open(dir.path().join("records.darudb")).unwrap();
        let mut rng = Rng::new(200 + seed);
        let as_encoded = crate::testing::TYPED_AS_ENCODED.get();
        let mut written = 0;

        for _ in 0..20 {
            let mut left = typed.begin_write().unwrap();
            let mut right = through_records.begin_write().unwrap();
            let open = left.schema().cloned().unwrap();

            for _ in 0..30 {
                let position = rng.index(2);
                let definition = &open.schema.collections[position];
                let declared = if position == 0 {
                    flat_people()
                } else {
                    flat_tags()
                }
                .all_fields();
                let kinds: Vec<&str> = declared
                    .iter()
                    .map(|field| match field.kind {
                        Type::String => "string",
                        Type::Int => "int",
                        Type::Float => "float",
                        Type::Bool => "bool",
                        _ => "bytes",
                    })
                    .collect();
                let values = random_values(&mut rng, &kinds, definition.auto);
                let replace = rng.below(2) == 0;
                // The record the typed writer sent before it encoded records
                // in the file's form, as a binding sends one: the fields the
                // object has, in id order.
                let record = codec::write(
                    &definition
                        .fields
                        .list
                        .iter()
                        .filter_map(|field| {
                            let slot = declared
                                .iter()
                                .position(|declared| declared.name == field.name)
                                .unwrap();

                            values[slot].clone().map(|value| (field.id, value))
                        })
                        .collect::<Vec<_>>(),
                );
                let fast = if position == 0 {
                    let mut people = left.collection_of::<FlatPerson>().unwrap();
                    let person = FlatPerson(values);

                    if replace {
                        people.put(&person)
                    } else {
                        people.insert(&person)
                    }
                    .map(Value::Int)
                } else {
                    let mut tags = left.collection_of::<FlatTag>().unwrap();
                    let tag = FlatTag(values);

                    if replace {
                        tags.put(&tag)
                    } else {
                        tags.insert(&tag)
                    }
                    .map(Value::String)
                };
                let slow = {
                    let mut collection = right.collection(&definition.name).unwrap();

                    if replace {
                        collection.put_record(&record)
                    } else {
                        collection.insert_record(&record)
                    }
                };

                match (&fast, &slow) {
                    (Ok(fast), Ok(slow)) => {
                        assert_eq!(fast, slow, "seed {seed}");
                        written += 1;
                    }
                    (Err(fast), Err(slow)) => {
                        assert_eq!(fast.to_string(), slow.to_string(), "seed {seed}");
                    }
                    _ => panic!(
                        "seed {seed}: {record:?} gave {fast:?} typed and {slow:?} as a record"
                    ),
                }
            }

            for collection in &open.schema.collections {
                let trees = std::iter::once(records(collection.id))
                    .chain(collection.indexes.iter().map(|index| index_tree(index.id)));

                for tree in trees {
                    let entries = |txn: &crate::WriteTransaction| {
                        txn.range_in::<Vec<u8>>(
                            &tree,
                            &(Bound::<Vec<u8>>::Unbounded, Bound::<Vec<u8>>::Unbounded),
                            false,
                        )
                        .unwrap()
                        .collect::<crate::Result<Vec<_>>>()
                        .unwrap()
                    };

                    assert_eq!(
                        entries(&left),
                        entries(&right),
                        "seed {seed}: tree {:?}",
                        &*tree
                    );
                }

                let next = |txn: &crate::WriteTransaction| {
                    txn.get_in(META, counter(collection.id).as_bytes()).unwrap()
                };

                assert_eq!(next(&left), next(&right), "seed {seed}");
            }

            check_indexes(&left as &dyn Source, &open.schema)
                .unwrap_or_else(|reason| panic!("seed {seed}: {reason}"));
            left.commit().unwrap();
            right.commit().unwrap();
        }

        assert!(written > 100, "seed {seed}: only {written} objects written");
        assert!(
            crate::testing::TYPED_AS_ENCODED.get() - as_encoded > 100,
            "seed {seed}: few typed writes stored as encoded"
        );
    }
}

/// Changes as a binding might send them for an update of an object whose
/// fields are `fields`, by id and kind: a few fields at random, now and then
/// made null, of the wrong type, an id the collection does not have, or
/// bytes cut or changed. The key field, the first, is left to the caller.
fn random_changes(rng: &mut Rng, fields: &[(u64, &str)]) -> Vec<(u64, Option<Raw>)> {
    let mut changes = Vec::new();

    for (id, kind) in &fields[1..] {
        if rng.below(3) != 0 {
            continue;
        }

        if rng.below(5) == 0 {
            changes.push((*id, None));

            continue;
        }

        let kind = if rng.below(30) == 0 {
            ["string", "int", "float", "bool", "bytes"][rng.index(5)]
        } else {
            kind
        };
        let value = match kind {
            "string" => {
                Raw::String(["ace", "bee", "cat", "", "a\0b", "é"][rng.index(6)].to_owned())
            }
            "int" => Raw::Int(i64::try_from(rng.below(8)).unwrap() - 2),
            "float" => Raw::Float([0.5, -1.0, 1e300][rng.index(3)]),
            "bool" => Raw::Bool(rng.below(2) == 0),
            _ => {
                let len = rng.index(4);

                Raw::Bytes(rng.bytes(len))
            }
        };

        changes.push((*id, Some(value)));
    }

    if rng.below(30) == 0 {
        changes.push((90 + rng.below(3), Some(Raw::Int(1))));
    }

    changes
}

/// An update of a record where it lies, as a binding's changes take it,
/// writes what reading the object, setting the fields changed and putting
/// it writes, and fails where that fails, with the same code: the same
/// records, byte for byte, and the same index entries.
#[test]
fn an_update_writes_what_a_put_of_the_changed_object_writes() {
    use crate::format::object::codec;

    let key_of = |position: usize, rng: &mut Rng| {
        if position == 0 {
            Value::Int(1 + i64::try_from(rng.below(30)).unwrap())
        } else {
            Value::String(["ace", "bee", "cat", "dot"][rng.index(4)].to_owned())
        }
    };

    for seed in 0..6 {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(flat_schema());

        let through_records = options.open(dir.path().join("records.darudb")).unwrap();
        let through_objects = options.open(dir.path().join("objects.darudb")).unwrap();
        let mut rng = Rng::new(100 + seed);
        let mut changed = 0;
        let mut missing = 0;

        for _ in 0..20 {
            let mut left = through_records.begin_write().unwrap();
            let mut right = through_objects.begin_write().unwrap();
            let open = left.schema().cloned().unwrap();

            for _ in 0..30 {
                let position = rng.index(2);
                let definition = &open.schema.collections[position];
                let name = definition.name.clone();
                let kinds: Vec<(u64, &str)> = definition
                    .fields
                    .list
                    .iter()
                    .map(|field| {
                        let kind = match field.kind {
                            Kind::String => "string",
                            Kind::Int => "int",
                            Kind::Float => "float",
                            Kind::Bool => "bool",
                            _ => "bytes",
                        };

                        (field.id, kind)
                    })
                    .collect();
                // The key field comes first in both collections' lists.
                assert_eq!(definition.fields.list[0].id, definition.key);

                // Objects to change, put the same way on both sides.
                if rng.below(3) == 0 {
                    let mut record =
                        codec::read(&random_record(&mut rng, &kinds)).unwrap_or_default();

                    record.retain(|(id, _)| *id != definition.key);
                    record.insert(
                        0,
                        (
                            definition.key,
                            match key_of(position, &mut rng) {
                                Value::Int(key) => Raw::Int(key),
                                Value::String(key) => Raw::String(key),
                                _ => unreachable!(),
                            },
                        ),
                    );

                    let record = codec::write(&record);
                    let fast = left.collection(&name).unwrap().put_record(&record);
                    let slow = right.collection(&name).unwrap().put_record(&record);

                    assert_eq!(fast.is_ok(), slow.is_ok(), "seed {seed}");

                    continue;
                }

                let key = key_of(position, &mut rng);
                let mut changes = random_changes(&mut rng, &kinds);

                // Now and then the key itself, as it is or another.
                if rng.below(10) == 0 {
                    let given = if rng.below(2) == 0 {
                        key.clone()
                    } else {
                        key_of(position, &mut rng)
                    };

                    changes.insert(
                        0,
                        (
                            definition.key,
                            Some(match given {
                                Value::Int(key) => Raw::Int(key),
                                Value::String(key) => Raw::String(key),
                                _ => unreachable!(),
                            }),
                        ),
                    );
                }

                let mut bytes = codec::write_changes(&changes);

                match rng.below(40) {
                    0 => bytes.truncate(rng.index(bytes.len())),
                    1 => {
                        let at = rng.index(bytes.len());

                        bytes[at] = u8::try_from(rng.below(256)).unwrap();
                    }
                    _ => {}
                }

                let fast = left
                    .collection(&name)
                    .unwrap()
                    .update_record(key.clone(), &bytes);
                // The model: read the object, set the fields, put it.
                let slow = match codec::changes_object_of(&bytes, &definition.fields) {
                    Err(reason) => Err(crate::Error::InvalidArgument {
                        message: reason.to_owned(),
                    }),
                    Ok(changes)
                        if changes
                            .get(&definition.fields.list[0].name)
                            .is_some_and(|given| *given != key) =>
                    {
                        Err(crate::Error::InvalidArgument {
                            message: "changes the key".to_owned(),
                        })
                    }
                    Ok(changes) => {
                        let mut collection = right.collection(&name).unwrap();

                        match collection.get(key.clone()).unwrap() {
                            None => Ok(false),
                            Some(mut object) => {
                                for (field, value) in changes.fields() {
                                    object.set(field, value.clone());
                                }

                                collection.put(object).map(|_| true)
                            }
                        }
                    }
                };

                match (&fast, &slow) {
                    (Ok(fast), Ok(slow)) => {
                        assert_eq!(fast, slow, "seed {seed}: {changes:?}");

                        if *fast {
                            changed += 1;
                        } else {
                            missing += 1;
                        }
                    }
                    (Err(fast), Err(slow)) => {
                        assert_eq!(fast.code(), slow.code(), "seed {seed}: {fast} and {slow}");
                    }
                    _ => panic!(
                        "seed {seed}: {changes:?} to {key:?} gave {fast:?} one way and {slow:?} the other"
                    ),
                }
            }

            for collection in &open.schema.collections {
                let trees = std::iter::once(records(collection.id))
                    .chain(collection.indexes.iter().map(|index| index_tree(index.id)));

                for tree in trees {
                    let entries = |txn: &crate::WriteTransaction| {
                        txn.range_in::<Vec<u8>>(
                            &tree,
                            &(Bound::<Vec<u8>>::Unbounded, Bound::<Vec<u8>>::Unbounded),
                            false,
                        )
                        .unwrap()
                        .collect::<crate::Result<Vec<_>>>()
                        .unwrap()
                    };

                    assert_eq!(
                        entries(&left),
                        entries(&right),
                        "seed {seed}: tree {:?}",
                        &*tree
                    );
                }
            }

            check_indexes(&left as &dyn Source, &open.schema)
                .unwrap_or_else(|reason| panic!("seed {seed}: {reason}"));
            left.commit().unwrap();
            right.commit().unwrap();
        }

        assert!(
            changed > 100 && missing > 10,
            "seed {seed}: {changed} objects changed, {missing} missing"
        );
    }
}

/// Deleting an object whose record is damaged fails with `CORRUPTED`, takes
/// nothing out, and leaves the transaction able to commit: the record is
/// read on its way out of the tree, and the removal stops there.
#[test]
fn deleting_a_damaged_object_takes_nothing_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = OpenOptions::new();

    options.schema(schema());

    let db = options.open(dir.path().join("objects.darudb")).unwrap();
    let mut txn = db.begin_write().unwrap();
    let open = txn.schema().cloned().unwrap();
    let players = open
        .schema
        .collections
        .iter()
        .find(|collection| collection.name == "players")
        .unwrap();
    let key = crate::format::object::key::encoded(&Value::Int(2)).unwrap();

    txn.collection("players")
        .unwrap()
        .insert(Object::new().with("id", 1).with("score", 3))
        .unwrap();
    // A record that ends inside its count of fields.
    txn.insert_in(&records(players.id), &key, &[0x80]).unwrap();

    let error = txn.collection("players").unwrap().delete(2).unwrap_err();

    assert_eq!(error.code(), "CORRUPTED");
    assert!(
        txn.get_in(&records(players.id), &key).unwrap().is_some(),
        "the damaged record was taken out"
    );
    assert!(txn.collection("players").unwrap().delete(1).unwrap());
    txn.commit().unwrap();
}

/// Updating an object whose record is damaged, or lacks a required field as
/// only a damaged record can, fails with `CORRUPTED`, changes nothing, and
/// leaves the transaction able to commit.
#[test]
fn updating_a_damaged_object_changes_nothing() {
    use crate::format::object::codec;

    let dir = tempfile::tempdir().unwrap();
    let mut options = OpenOptions::new();

    options.schema(flat_schema());

    let db = options.open(dir.path().join("objects.darudb")).unwrap();
    let mut txn = db.begin_write().unwrap();
    let open = txn.schema().cloned().unwrap();
    let people = &open.schema.collections[0];
    let key = |id: i64| crate::format::object::key::encoded(&Value::Int(id)).unwrap();

    txn.collection("people")
        .unwrap()
        .insert(Object::new().with("name", "Ann"))
        .unwrap();
    // A record that ends inside its count of fields, and one without `name`.
    txn.insert_in(&records(people.id), &key(2), &[0x80])
        .unwrap();
    txn.insert_in(
        &records(people.id),
        &key(3),
        &codec::write(&[(1, Raw::Int(3)), (4, Raw::Int(40))]),
    )
    .unwrap();

    let name = codec::write_changes(&[(2, Some(Raw::String("Bo".to_owned())))]);
    let age = codec::write_changes(&[(4, Some(Raw::Int(41)))]);

    for (id, changes) in [(2, &age), (3, &age), (3, &name)] {
        let mut collection = txn.collection("people").unwrap();
        let before = collection.get_record(id).unwrap();
        let error = collection.update_record(id, changes).unwrap_err();

        assert_eq!(error.code(), "CORRUPTED", "{id}");
        assert_eq!(collection.get_record(id).unwrap(), before, "{id}");
    }

    assert!(
        txn.collection("people")
            .unwrap()
            .update_record(1, &age)
            .unwrap()
    );
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();

    assert_eq!(
        read.collection("people")
            .unwrap()
            .get(1)
            .unwrap()
            .unwrap()
            .get("age"),
        Some(&Value::Int(41))
    );
}

/// A put refused after the transaction copied its way down the collection's
/// tree, as the first change of the transaction to that tree, leaves the
/// tree on the copies: the commit has to write them, or the tree it leaves
/// behind is on pages given up for later commits to reuse. A put over a
/// damaged record is refused on the way down; one that takes a unique value
/// is refused after its record was stored, and puts the record back.
#[test]
fn a_put_refused_as_the_first_change_leaves_a_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = OpenOptions::new();

    options.schema(schema());

    let db = options.open(dir.path().join("objects.darudb")).unwrap();
    let open = db.begin_read().unwrap().schema().cloned().unwrap();
    let players = open
        .schema
        .collections
        .iter()
        .find(|collection| collection.name == "players")
        .unwrap();
    let tree = records(players.id);
    let damaged = crate::format::object::key::encoded(&Value::Int(2001)).unwrap();
    let write = |round: i64| {
        let mut txn = db.begin_write().unwrap();
        let mut players = txn.collection("players").unwrap();

        for n in 1..=2000 {
            let handle = format!("h{n}");

            players
                .put(
                    Object::new()
                        .with("id", n)
                        .with("handle", handle)
                        .with("score", round),
                )
                .unwrap();
        }

        drop(players);
        txn.commit().unwrap();
    };

    write(0);

    let mut txn = db.begin_write().unwrap();

    // A record that ends inside its count of fields.
    txn.insert_in(&tree, &damaged, &[0x80]).unwrap();
    txn.commit().unwrap();

    for (id, handle, code) in [(2001, "new", "CORRUPTED"), (5, "h6", "DUPLICATE_KEY")] {
        let mut txn = db.begin_write().unwrap();
        let refused = txn
            .collection("players")
            .unwrap()
            .put(Object::new().with("id", id).with("handle", handle))
            .unwrap_err();

        assert_eq!(refused.code(), code);
        txn.commit().unwrap();

        // Commits that reuse the pages the refused put gave up.
        for round in 1..4 {
            write(round);
        }

        let read = db.begin_read().unwrap();
        let stored = read
            .range_in::<Vec<u8>>(
                &tree,
                &(Bound::<Vec<u8>>::Unbounded, Bound::<Vec<u8>>::Unbounded),
                false,
            )
            .unwrap()
            .collect::<crate::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(stored.len(), 2001, "{code}");
    }
}
