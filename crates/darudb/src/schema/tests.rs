//! Tests of the object layer that look below the public API: the index trees
//! against the objects they index.

use super::objects::{Source, check_indexes};
use super::{Collection, Schema, Type};
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
