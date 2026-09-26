//! Tests of the object layer that look below the public API: the index trees
//! against the objects they index.

use std::ops::Bound;

use super::objects::{Source, check_indexes, index_tree, records};
use super::{Collection, Schema, Type};
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

        for round in 0..30 {
            let mut txn = db.begin_write().unwrap();
            let open = txn.schema().cloned().unwrap();

            for _ in 0..40 {
                let mut players = txn.collection("players").unwrap();
                let result = match rng.below(10) {
                    0..=4 => players.insert(player(&mut rng)).map(drop),
                    5..=7 => players.put(player(&mut rng)).map(drop),
                    _ => players
                        .delete(1 + i64::try_from(rng.below(40)).unwrap())
                        .map(drop),
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
        .collection(
            Collection::new("people")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 18)
                .optional("score", Type::Float)
                .with_default("admin", Type::Bool, false)
                .optional("photo", Type::Bytes)
                .unique("email")
                .index("age")
                .index("admin"),
        )
        .collection(
            Collection::new("tags")
                .primary_key("label", Type::String)
                .with_default("uses", Type::Int, 0)
                .index("uses"),
        )
}

/// A record as a binding might send one for a collection whose fields are
/// `fields`, by id and kind: most fields at random, now and then a value of
/// the wrong type, an id the collection does not have, the key, or bytes
/// cut or changed.
fn random_record(rng: &mut Rng, fields: &[(u64, &str)]) -> Vec<u8> {
    use crate::format::object::codec::{self, Raw};

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
