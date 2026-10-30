//! Tests of the object layer that look below the public API: the index trees
//! against the objects they index.

use std::ops::Bound;

use super::objects::{Source, check_indexes};
use super::typed::{CollectionType, FieldReader, ValueWriter};
use super::{Collection, Embedded, Schema, Type};
use crate::format::object::codec::Raw;
use crate::format::object::names::{index_tree, records};
use crate::format::object::schema::{Kind, OBJECT_FORMAT, StoredSchema};
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

/// A handle opened without a schema on a file whose stored schema does not
/// read opens all the same, for the tools, and says why it has no
/// collections when one is reached: `CORRUPTED` for a damaged record, and
/// `UNSUPPORTED_FORMAT_VERSION` for an object format this build does not know.
#[test]
fn a_stored_schema_that_does_not_read_leaves_a_handle_without_one() {
    use crate::format::object::codec;
    use crate::format::object::names::{META, SCHEMA_KEY};

    let damaged = vec![0x80];
    let newer = codec::write(&[(1, Raw::Int(OBJECT_FORMAT + 1))]);

    for (record, expected) in [
        (damaged, "CORRUPTED"),
        (newer, "UNSUPPORTED_FORMAT_VERSION"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects.darudb");

        {
            let db = OpenOptions::new().schema(schema()).open(&path).unwrap();
            let mut txn = db.begin_write().unwrap();

            txn.insert_in(META, SCHEMA_KEY, &record).unwrap();
            txn.commit().unwrap();
        }

        let db = Database::open(&path).unwrap();

        assert!(db.stored_schema().is_none());
        assert!(db.schema_record().is_none());
        assert_eq!(
            db.begin_read()
                .unwrap()
                .collection("players")
                .err()
                .map(|error| error.code()),
            Some(expected)
        );
        // The integrity check still runs, and names the schema.
        assert!(
            db.check()
                .unwrap()
                .problems
                .iter()
                .any(|problem| problem.message.contains("its schema")),
            "the check missed the damaged schema"
        );
    }
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

/// A collection with a list, an embedded object and links beside its
/// scalars, whose indexes take the list and a link too.
fn mixed_schema() -> Schema {
    Schema::new(1)
        .collection(Collection::new("teams").primary_key("name", Type::String))
        .collection(
            Collection::new("players")
                .with_default("score", Type::Int, 0)
                .optional("handle", Type::String)
                .optional("rating", Type::Float)
                .optional("tags", Type::list(Type::String))
                .optional("team", Type::link("teams"))
                .optional(
                    "card",
                    Type::object(
                        super::Embedded::new()
                            .field("city", Type::String)
                            .optional("zip", Type::Int),
                    ),
                )
                .index("score")
                .unique("handle")
                .index("tags")
                .index("team"),
        )
}

/// An update of the scalars of an object that also holds a list, an
/// embedded object and links changes the record where it lies, copying the
/// rest, and writes what reading the object, setting the fields and putting
/// it writes: the same records, byte for byte, and the same index entries.
/// Changes of the list or the embedded object, which go through objects,
/// come between them.
#[test]
fn an_update_of_scalars_beside_other_fields_writes_what_a_put_writes() {
    use crate::format::object::codec;

    for seed in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(mixed_schema());

        let through_records = options.open(dir.path().join("records.darudb")).unwrap();
        let through_objects = options.open(dir.path().join("objects.darudb")).unwrap();
        let mut rng = Rng::new(300 + seed);
        let pick = |rng: &mut Rng, of: &[&str]| of[rng.index(of.len())].to_owned();

        for db in [&through_records, &through_objects] {
            let mut txn = db.begin_write().unwrap();
            let mut teams = txn.collection("teams").unwrap();

            teams.insert(Object::new().with("name", "north")).unwrap();
            teams.insert(Object::new().with("name", "south")).unwrap();
            txn.commit().unwrap();
        }

        let open = through_records
            .begin_read()
            .unwrap()
            .schema()
            .cloned()
            .unwrap();
        let players = &open.schema.collections[1];
        let id = |name: &str| {
            players
                .fields
                .list
                .iter()
                .find(|field| field.name == name)
                .unwrap()
                .id
        };
        let mut changed = 0;

        for _ in 0..10 {
            let mut left = through_records.begin_write().unwrap();
            let mut right = through_objects.begin_write().unwrap();

            for _ in 0..30 {
                let key = 1 + i64::try_from(rng.below(25)).unwrap();

                // Objects to change, put the same way on both sides.
                if rng.below(4) == 0 {
                    let mut object = Object::new()
                        .with("id", key)
                        .with("score", i64::try_from(rng.below(5)).unwrap());

                    if rng.below(2) == 0 {
                        let tags = (0..rng.below(4))
                            .map(|_| Value::from(pick(&mut rng, &["red", "blue", "red"])))
                            .collect::<Vec<_>>();

                        object.set("tags", tags);
                    }

                    if rng.below(2) == 0 {
                        object.set("team", pick(&mut rng, &["north", "south"]));
                    }

                    if rng.below(2) == 0 {
                        let mut card = Object::new().with("city", pick(&mut rng, &["Oslo", "Ulm"]));

                        if rng.below(2) == 0 {
                            card.set("zip", 1000);
                        }

                        object.set("card", card);
                    }

                    left.collection("players")
                        .unwrap()
                        .put(object.clone())
                        .unwrap();
                    right.collection("players").unwrap().put(object).unwrap();

                    continue;
                }

                let mut changes = Vec::new();

                if rng.below(2) == 0 {
                    changes.push((
                        id("score"),
                        Some(Raw::Int(i64::try_from(rng.below(5)).unwrap())),
                    ));
                }

                if rng.below(3) == 0 {
                    let handle = (rng.below(4) > 0)
                        .then(|| Raw::String(pick(&mut rng, &["ace", "bee", "cat"])));

                    changes.push((id("handle"), handle));
                }

                if rng.below(3) == 0 {
                    changes.push((id("rating"), Some(Raw::Float([0.5, 2.0][rng.index(2)]))));
                }

                match rng.below(8) {
                    0 => changes.push((
                        id("tags"),
                        Some(Raw::List(vec![Raw::String("blue".to_owned())])),
                    )),
                    1 => changes.push((id("card"), None)),
                    _ => {}
                }

                changes.sort_by_key(|(id, _)| *id);

                let bytes = codec::write_changes(&changes);
                let fast = left
                    .collection("players")
                    .unwrap()
                    .update_record(key, &bytes);
                let slow = {
                    let changes = codec::changes_object_of(&bytes, &players.fields).unwrap();
                    let mut collection = right.collection("players").unwrap();

                    match collection.get(key).unwrap() {
                        None => Ok(false),
                        Some(mut object) => {
                            for (field, value) in changes.fields() {
                                object.set(field, value.clone());
                            }

                            collection.put(object).map(|_| true)
                        }
                    }
                };

                match (&fast, &slow) {
                    (Ok(fast), Ok(slow)) => {
                        assert_eq!(fast, slow, "seed {seed}: {changes:?}");
                        changed += usize::from(*fast);
                    }
                    (Err(fast), Err(slow)) => {
                        assert_eq!(fast.code(), slow.code(), "seed {seed}: {fast} and {slow}");
                    }
                    _ => panic!(
                        "seed {seed}: {changes:?} gave {fast:?} one way and {slow:?} the other"
                    ),
                }
            }

            let trees = std::iter::once(records(players.id))
                .chain(players.indexes.iter().map(|index| index_tree(index.id)));

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

            check_indexes(&left as &dyn Source, &open.schema)
                .unwrap_or_else(|reason| panic!("seed {seed}: {reason}"));
            left.commit().unwrap();
            right.commit().unwrap();
        }

        assert!(changed > 50, "seed {seed}: {changed} objects changed");
    }
}

/// An update of a scalar of an object whose embedded object is damaged
/// fails with `CORRUPTED` and changes nothing, as reading the object would
/// find it damaged: the values it copies are read whole first.
#[test]
fn an_update_beside_a_damaged_embedded_object_changes_nothing() {
    use crate::format::object::codec;

    let dir = tempfile::tempdir().unwrap();
    let mut options = OpenOptions::new();

    options.schema(mixed_schema());

    let db = options.open(dir.path().join("objects.darudb")).unwrap();
    let mut txn = db.begin_write().unwrap();
    let open = txn.schema().cloned().unwrap();
    let players = &open.schema.collections[1];
    let field = |name: &str| {
        players
            .fields
            .list
            .iter()
            .find(|field| field.name == name)
            .unwrap()
            .id
    };
    let key = crate::format::object::key::encoded(&Value::Int(1)).unwrap();

    // A card without its required `city`.
    txn.insert_in(
        &records(players.id),
        &key,
        &codec::write(&[
            (field("id"), Raw::Int(1)),
            (field("score"), Raw::Int(2)),
            (field("card"), Raw::Object(vec![(2, Raw::Int(5))])),
        ]),
    )
    .unwrap();

    let score = codec::write_changes(&[(field("score"), Some(Raw::Int(3)))]);
    let mut collection = txn.collection("players").unwrap();
    let before = collection.get_record(1).unwrap();
    let error = collection.update_record(1, &score).unwrap_err();

    assert_eq!(error.code(), "CORRUPTED");
    assert_eq!(collection.get_record(1).unwrap(), before);
    drop(collection);
    txn.commit().unwrap();
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

/// The object format the stored schema of `db` is in.
fn object_format(db: &Database) -> i64 {
    use crate::format::object::codec;

    match codec::read(db.schema_record().unwrap()).unwrap()[0] {
        (1, Raw::Int(format)) => format,
        ref other => panic!("the schema's first field is {other:?}"),
    }
}

/// A schema of `people` at `version`, with a date field `born` when `dated`.
fn people(version: u64, dated: bool) -> Schema {
    let people = Collection::new("people").field("name", Type::String);

    Schema::new(version).collection(if dated {
        people.optional("born", Type::Date).index("born")
    } else {
        people
    })
}

/// The first date a schema declares raises the file to object format 2, and
/// the file stays there when a later version drops the date: its records
/// still hold dates, which a build of format 1 could not read.
#[test]
fn the_first_date_raises_the_object_format_and_nothing_lowers_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");

    {
        let db = OpenOptions::new()
            .schema(people(1, false))
            .open(&path)
            .unwrap();

        assert_eq!(object_format(&db), 1);
    }

    {
        let db = OpenOptions::new()
            .schema(people(2, true))
            .open(&path)
            .unwrap();
        let mut txn = db.begin_write().unwrap();

        assert_eq!(object_format(&db), 2);
        txn.collection("people")
            .unwrap()
            .insert(
                Object::new()
                    .with("name", "Ada")
                    .with("born", Value::Date(-1)),
            )
            .unwrap();
        txn.commit().unwrap();
    }

    let db = OpenOptions::new()
        .schema(people(3, false))
        .open(&path)
        .unwrap();

    assert_eq!(object_format(&db), 2);
    assert_eq!(
        db.begin_read()
            .unwrap()
            .collection("people")
            .unwrap()
            .get(1)
            .unwrap()
            .unwrap()
            .get("born"),
        None
    );
    drop(db);

    // The same declaration opens the file again unchanged.
    let db = OpenOptions::new()
        .schema(people(3, false))
        .open(&path)
        .unwrap();

    assert_eq!(object_format(&db), 2);
    assert!(db.check().unwrap().is_ok());
}

/// `upgrade_format(false)` keeps a file in the object format it has, for
/// the builds before format 2: a schema that needs format 2 is refused,
/// with the file left as it was, and a file already in format 2 opens.
#[test]
fn a_file_kept_from_upgrading_stays_in_object_format_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let kept = |schema: Schema| {
        let mut options = OpenOptions::new();

        options.schema(schema).upgrade_format(false);
        options.open(&path)
    };

    let refused = kept(people(1, true)).err().unwrap();

    assert_eq!(refused.code(), "INVALID_ARGUMENT");
    assert!(refused.to_string().contains("upgrade_format"), "{refused}");

    // The file made for it holds no schema, and takes one of format 1.
    drop(kept(people(1, false)).unwrap());
    assert_eq!(
        kept(people(2, true)).err().map(|error| error.code()),
        Some("INVALID_ARGUMENT")
    );
    assert_eq!(object_format(&kept(people(1, false)).unwrap()), 1);

    // Allowed, the migration raises it, and a handle kept from upgrading
    // opens the file of format 2 as it is.
    drop(
        OpenOptions::new()
            .schema(people(2, true))
            .open(&path)
            .unwrap(),
    );
    assert_eq!(object_format(&kept(people(2, true)).unwrap()), 2);
}

/// A stored schema in an object format this build does not know is told
/// apart from damage everywhere: opening with a schema refuses it, the
/// message names the object format, and salvage refuses to repair under it
/// rather than dropping every object, with nothing written.
#[test]
fn an_object_format_this_build_does_not_read_is_refused_and_not_salvaged() {
    use crate::format::object::codec;
    use crate::format::object::names::{META, SCHEMA_KEY};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");

    {
        let db = OpenOptions::new()
            .schema(people(1, true))
            .open(&path)
            .unwrap();
        let mut raw = codec::read(db.schema_record().unwrap()).unwrap();
        let mut txn = db.begin_write().unwrap();

        raw[0].1 = Raw::Int(OBJECT_FORMAT + 1);
        txn.insert_in(META, SCHEMA_KEY, &codec::write(&raw))
            .unwrap();
        txn.commit().unwrap();
    }

    let refused = OpenOptions::new()
        .schema(people(1, true))
        .open(&path)
        .err()
        .unwrap();

    assert_eq!(refused.code(), "UNSUPPORTED_FORMAT_VERSION");
    assert!(
        refused
            .to_string()
            .contains(&format!("object format {}", OBJECT_FORMAT + 1)),
        "{refused}"
    );

    let into = dir.path().join("salvaged.darudb");

    assert_eq!(
        OpenOptions::new()
            .salvage(&path, &into)
            .err()
            .map(|error| error.code()),
        Some("UNSUPPORTED_FORMAT_VERSION")
    );
    assert!(!into.exists());
}

/// Trips with a list of stops, each an embedded object, at `version`, the
/// stops gaining a zip code in version 2.
fn trips(version: u64) -> Schema {
    let stop = Embedded::new().field("city", Type::String);
    let stop = if version >= 2 {
        stop.optional("zip", Type::String)
    } else {
        stop
    };

    Schema::new(version).collection(
        Collection::new("trips")
            .field("name", Type::String)
            .with_default(
                "stops",
                Type::list(Type::object(stop)),
                Value::List(Vec::new()),
            ),
    )
}

/// A list of embedded objects is object format 2, its elements' fields keep
/// their ids from version to version as a collection's do, and a record
/// written before an element gained a field reads it as null.
#[test]
fn a_list_of_embedded_objects_keeps_its_elements_fields_by_name() {
    use crate::format::object::codec;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.darudb");
    let stop = |city: &str| Value::Object(Object::new().with("city", city));
    let element_ids = |db: &Database| {
        let schema = StoredSchema::decode(db.schema_record().unwrap()).unwrap();
        let stops = schema.collections[0].fields.by_name("stops").unwrap();
        let Kind::List(element) = &stops.kind else {
            panic!("{stops:?}")
        };
        let Kind::Object(fields) = element.as_ref() else {
            panic!("{element:?}")
        };

        fields
            .list
            .iter()
            .map(|field| (field.name.clone(), field.id))
            .collect::<Vec<_>>()
    };

    {
        let db = OpenOptions::new().schema(trips(1)).open(&path).unwrap();
        let mut txn = db.begin_write().unwrap();

        assert_eq!(object_format(&db), 2);
        txn.collection("trips")
            .unwrap()
            .insert(
                Object::new()
                    .with("name", "coast")
                    .with("stops", vec![stop("Busan"), stop("Ulsan")]),
            )
            .unwrap();
        txn.commit().unwrap();
        assert_eq!(element_ids(&db), [("city".to_owned(), 1)]);
    }

    let db = OpenOptions::new().schema(trips(2)).open(&path).unwrap();

    assert_eq!(
        element_ids(&db),
        [("city".to_owned(), 1), ("zip".to_owned(), 2)]
    );

    let mut txn = db.begin_write().unwrap();
    let mut trips_of = txn.collection("trips").unwrap();
    let first = trips_of.get(1).unwrap().unwrap();

    assert_eq!(
        first.get("stops"),
        Some(&Value::List(vec![
            Value::Object(Object::new().with("city", "Busan").with("zip", Value::Null)),
            Value::Object(Object::new().with("city", "Ulsan").with("zip", Value::Null)),
        ]))
    );
    trips_of
        .insert(Object::new().with("name", "empty"))
        .unwrap();
    assert_eq!(
        trips_of.get(2).unwrap().unwrap().get("stops"),
        Some(&Value::List(Vec::new()))
    );
    assert_eq!(
        trips_of
            .count(&crate::Query::new().filter(crate::Filter::eq("stops.city", "Ulsan")))
            .unwrap(),
        1
    );
    // A stop holds an object, and nothing else.
    assert_eq!(
        trips_of
            .insert(
                Object::new()
                    .with("name", "x")
                    .with("stops", vec![Value::from("Busan")])
            )
            .err()
            .map(|error| error.code()),
        Some("INVALID_ARGUMENT")
    );
    drop(trips_of);
    txn.commit().unwrap();

    // The element record holds the city under its id, as any record does.
    let raw = codec::read(
        &db.begin_read()
            .unwrap()
            .collection("trips")
            .unwrap()
            .get_record(1)
            .unwrap()
            .unwrap(),
    )
    .unwrap();

    assert!(
        raw.iter()
            .any(|(_, value)| matches!(value, Raw::List(stops) if stops.len() == 2))
    );
    assert!(db.check().unwrap().is_ok());
}

/// A default of a list of embedded objects is the empty list or nothing, and
/// a list of lists is no type at all.
#[test]
fn a_list_of_embedded_objects_takes_only_the_empty_list_as_a_default() {
    let dir = tempfile::tempdir().unwrap();
    let stop = || Type::object(Embedded::new().field("city", Type::String));
    let declared = |kind: Type, default: Value| {
        Schema::new(1).collection(Collection::new("trips").with_default("stops", kind, default))
    };
    let code = |schema: Schema| {
        OpenOptions::new()
            .schema(schema)
            .open(dir.path().join("refused.darudb"))
            .err()
            .map(|error| error.code())
    };

    assert_eq!(
        code(declared(
            Type::list(stop()),
            Value::List(vec![Value::Object(Object::new().with("city", "Busan"))])
        )),
        Some("INVALID_ARGUMENT")
    );
    assert_eq!(
        code(declared(
            Type::list(Type::list(Type::Int)),
            Value::List(Vec::new())
        )),
        Some("INVALID_ARGUMENT")
    );
}
