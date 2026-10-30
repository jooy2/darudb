//! Collections of objects through the public API: schemas, writes, unique
//! indexes and migrations.

// The helpers below fail the test that called them by panicking, the way a
// test does; `clippy.toml` allows that inside tests but not in helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::TestDir;
use darudb::{Collection, Database, Error, Migration, Object, OpenOptions, Schema, Type, Value};

fn users_v1() -> Collection {
    Collection::new("users")
        .field("name", Type::String)
        .optional("email", Type::String)
        .with_default("age", Type::Int, 0)
        .unique("email")
        .index("age")
}

fn posts_v1() -> Collection {
    Collection::new("posts")
        .primary_key("slug", Type::String)
        .field("author", Type::link("users"))
        .optional("tags", Type::list(Type::String))
        .index("author")
        .index("tags")
}

fn v1() -> Schema {
    Schema::new(1).collection(users_v1()).collection(posts_v1())
}

fn open(dir: &TestDir, schema: Schema, migrations: &[Migration]) -> darudb::Result<Database> {
    let mut options = OpenOptions::new();

    options.schema(schema);

    for migration in migrations {
        options.migration(migration.clone());
    }

    options.open(dir.path("app.darudb"))
}

/// Opens the file at `path` with `schema`.
fn open_at(path: &std::path::Path, schema: Schema) -> darudb::Result<Database> {
    OpenOptions::new().schema(schema).open(path)
}

fn code<T>(result: darudb::Result<T>) -> &'static str {
    result.err().map_or("OK", |error| error.code())
}

fn user(name: &str) -> Object {
    Object::new().with("name", name)
}

/// Every object of `collection`, in key order.
fn objects(db: &Database, collection: &str) -> Vec<Object> {
    let read = db.begin_read().unwrap();
    let reader = read.collection(collection).unwrap();

    reader.iter().unwrap().map(Result::unwrap).collect()
}

/// A file at `v1` with two users and a post.
fn with_data(dir: &TestDir) {
    let db = open(dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("users")
        .unwrap()
        .insert(
            user("Alice")
                .with("email", "alice@example.com")
                .with("age", 30),
        )
        .unwrap();
    txn.collection("users")
        .unwrap()
        .insert(user("Bob"))
        .unwrap();
    txn.collection("posts")
        .unwrap()
        .insert(
            Object::new()
                .with("slug", "hello")
                .with("author", 1)
                .with("tags", vec![Value::from("intro")]),
        )
        .unwrap();
    txn.commit().unwrap();
}

#[test]
fn objects_are_there_after_the_file_is_opened_again() {
    let dir = TestDir::new();

    with_data(&dir);

    let db = open(&dir, v1(), &[]).unwrap();
    let read = db.begin_read().unwrap();
    let users = read.collection("users").unwrap();

    assert_eq!(users.len().unwrap(), 2);
    assert_eq!(
        users.get(1).unwrap(),
        Some(
            user("Alice")
                .with("id", 1)
                .with("email", "alice@example.com")
                .with("age", 30)
        )
    );
    assert_eq!(
        users.get(2).unwrap(),
        Some(
            user("Bob")
                .with("id", 2)
                .with("email", Value::Null)
                .with("age", 0)
        ),
        "a left-out field holds its default, or null"
    );
    assert_eq!(users.get(3).unwrap(), None);
    assert_eq!(
        read.collection("posts")
            .unwrap()
            .get("hello")
            .unwrap()
            .unwrap()
            .get("author"),
        Some(&Value::Int(1))
    );
    assert_eq!(
        read.tree_names().unwrap(),
        Vec::<String>::new(),
        "the engine's trees are not the application's"
    );
}

/// An object read by key, by a query or by a walk has its fields in their
/// order by name, in each collection, when two collections have as many
/// fields in different orders, in a read transaction and in a write one.
#[test]
fn an_object_read_by_key_has_its_fields_by_name_in_every_collection() {
    let dir = TestDir::new();
    let schema = Schema::new(1)
        .collection(
            Collection::new("first")
                .field("b", Type::String)
                .field("a", Type::Int),
        )
        .collection(
            Collection::new("second")
                .field("x", Type::Int)
                .field("zeta", Type::String),
        );
    let db = open(&dir, schema, &[]).unwrap();
    let first = Object::new().with("a", 1).with("b", "one").with("id", 1);
    let second = Object::new().with("id", 1).with("x", 2).with("zeta", "two");
    let mut txn = db.begin_write().unwrap();

    txn.collection("first")
        .unwrap()
        .insert(first.clone())
        .unwrap();
    txn.collection("second")
        .unwrap()
        .insert(second.clone())
        .unwrap();

    assert_eq!(
        txn.collection("first").unwrap().get(1).unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        txn.collection("second").unwrap().get(1).unwrap(),
        Some(second.clone())
    );
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();

    for (name, expected) in [("first", &first), ("second", &second)] {
        let collection = read.collection(name).unwrap();
        let queried = collection.query(&darudb::Query::new()).unwrap();
        let walked: Vec<Object> = collection.iter().unwrap().map(Result::unwrap).collect();

        for object in [collection.get(1).unwrap().unwrap()]
            .iter()
            .chain(&queried)
            .chain(&walked)
        {
            let names: Vec<&str> = object.fields().map(|(name, _)| name).collect();

            assert_eq!(object, expected, "{name}");
            assert!(names.is_sorted(), "{name}: {names:?}");
            assert_eq!(object.get("id"), Some(&Value::Int(1)), "{name}");
        }

        assert_eq!(queried.len(), 1);
        assert_eq!(walked.len(), 1);
    }
}

/// An object stored before its collection had an indexed field is in the
/// index under the field's default, and changing or deleting the object
/// takes that entry out again.
#[test]
fn an_indexed_field_added_later_leaves_its_default_in_the_index_until_changed() {
    let dir = TestDir::new();
    let v1 = Schema::new(1).collection(Collection::new("things").field("name", Type::String));
    let db = open(&dir, v1, &[]).unwrap();
    let mut txn = db.begin_write().unwrap();

    for name in ["a", "b", "c"] {
        txn.collection("things")
            .unwrap()
            .insert(Object::new().with("name", name))
            .unwrap();
    }

    txn.commit().unwrap();
    drop(db);

    let v2 = Schema::new(2).collection(
        Collection::new("things")
            .field("name", Type::String)
            .with_default("rank", Type::Int, 5)
            .index("rank"),
    );
    let db = open(&dir, v2, &[]).unwrap();
    let ranked = |db: &Database, rank: i64| -> Vec<i64> {
        let read = db.begin_read().unwrap();

        read.collection("things")
            .unwrap()
            .query(&darudb::Query::new().filter(darudb::Filter::eq("rank", rank)))
            .unwrap()
            .iter()
            .map(|thing| thing.get("id").and_then(Value::as_int).unwrap())
            .collect()
    };

    assert_eq!(ranked(&db, 5), [1, 2, 3]);

    let mut txn = db.begin_write().unwrap();
    let mut things = txn.collection("things").unwrap();
    let mut first = things.get(1).unwrap().unwrap();

    first.set("rank", 7);
    things.put(first).unwrap();
    assert!(things.delete(2).unwrap());
    txn.commit().unwrap();

    assert_eq!(ranked(&db, 5), [3]);
    assert_eq!(ranked(&db, 7), [1]);
}

#[test]
fn an_auto_increment_never_gives_a_number_twice() {
    let dir = TestDir::new();
    let db = open(&dir, v1(), &[]).unwrap();
    let insert = |object: Object| {
        let mut txn = db.begin_write().unwrap();
        let key = txn.collection("users").unwrap().insert(object).unwrap();

        txn.commit().unwrap();

        key.as_int().unwrap()
    };

    assert_eq!(insert(user("a")), 1);
    assert_eq!(insert(user("b")), 2);

    let mut txn = db.begin_write().unwrap();

    assert!(txn.collection("users").unwrap().delete(2).unwrap());
    assert!(!txn.collection("users").unwrap().delete(2).unwrap());
    txn.commit().unwrap();

    assert_eq!(
        insert(user("c")),
        3,
        "a deleted object's number is not reused"
    );
    assert_eq!(insert(user("d").with("id", 10)), 10);
    assert_eq!(insert(user("e")), 11, "the next number passes a chosen one");
    assert_eq!(insert(user("f").with("id", 5)), 5);
    assert_eq!(
        insert(user("g")),
        12,
        "a lower chosen number changes nothing"
    );

    drop(db);

    let db = open(&dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();

    assert_eq!(
        txn.collection("users").unwrap().insert(user("h")).unwrap(),
        Value::Int(13)
    );

    // The counter is stored when the transaction commits: numbers given in
    // one transaction follow each other, and an aborted one gives none.
    assert_eq!(
        txn.collection("users").unwrap().insert(user("i")).unwrap(),
        Value::Int(14)
    );
    txn.abort();

    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();

    assert_eq!(
        users
            .insert(user("h").with("email", "h@example.com"))
            .unwrap(),
        Value::Int(13)
    );
    assert_eq!(
        code(users.insert(user("j").with("email", "h@example.com"))),
        "DUPLICATE_KEY",
        "the email is taken"
    );
    assert_eq!(
        users.insert(user("k")).unwrap(),
        Value::Int(14),
        "a refused insert takes no number"
    );
    drop(users);
    txn.commit().unwrap();

    let mut txn = db.begin_write().unwrap();

    assert_eq!(
        txn.collection("users").unwrap().insert(user("l")).unwrap(),
        Value::Int(15)
    );

    // A put with a number of its own raises the counter past it as an insert
    // does, unless it replaces an object, which is below the counter already.
    let mut users = txn.collection("users").unwrap();

    users.put(user("m").with("id", 3)).unwrap();
    assert_eq!(users.insert(user("n")).unwrap(), Value::Int(16));
    users.put(user("o").with("id", 30)).unwrap();
    assert_eq!(
        users.insert(user("p")).unwrap(),
        Value::Int(31),
        "a put that adds an object raises the counter"
    );
}

#[test]
fn a_refused_write_leaves_the_transaction_able_to_commit() {
    let dir = TestDir::new();

    with_data(&dir);

    let db = open(&dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();
    let long = "x".repeat(2000);

    users
        .put(user("Bob").with("id", 2).with("age", 40))
        .unwrap();

    assert_eq!(
        code(users.insert(user("Carol").with("id", 1))),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        code(users.insert(user("Carol").with("email", "alice@example.com"))),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        code(users.insert(Object::new())),
        "INVALID_ARGUMENT",
        "no name"
    );
    assert_eq!(
        code(users.insert(user("Carol").with("age", "old"))),
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        code(users.insert(user("Carol").with("id", "c"))),
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        code(users.insert(user("Carol").with("email", long.as_str()))),
        "INVALID_ARGUMENT"
    );
    // A put is refused the same way, whether it replaces an object or adds
    // one, and a refused replacement leaves the object it would replace.
    assert_eq!(
        code(users.put(user("Bob").with("id", 2).with("email", "alice@example.com"))),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        code(users.put(user("Dan").with("id", 9).with("email", "alice@example.com"))),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        code(users.put(user("Bob").with("id", 2).with("email", long.as_str()))),
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        users.get(2).unwrap().unwrap().get("age"),
        Some(&Value::Int(40))
    );
    assert_eq!(users.get(9).unwrap(), None);

    let mut posts = txn.collection("posts").unwrap();

    assert_eq!(
        code(posts.insert(Object::new().with("slug", long.as_str()).with("author", 1))),
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        code(posts.insert(Object::new().with("slug", "x").with("author", "alice"))),
        "INVALID_ARGUMENT",
        "a link holds the target's key type"
    );
    assert_eq!(code(posts.delete(7)), "INVALID_ARGUMENT");

    txn.commit().unwrap();

    assert_eq!(objects(&db, "users")[1].get("age"), Some(&Value::Int(40)));
    assert_eq!(objects(&db, "users").len(), 2);

    // The indexes hold what the objects do.
    let read = db.begin_read().unwrap();
    let users = read.collection("users").unwrap();
    let keys = |filter| {
        users
            .query(&darudb::Query::new().filter(filter))
            .unwrap()
            .iter()
            .map(|user| user.get("id").cloned())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        keys(darudb::Filter::eq("email", "alice@example.com")),
        [Some(Value::Int(1))]
    );
    assert_eq!(keys(darudb::Filter::eq("age", 40)), [Some(Value::Int(2))]);
}

#[test]
fn a_unique_index_allows_any_number_of_nulls_and_frees_a_value_it_no_longer_holds() {
    let dir = TestDir::new();
    let db = open(&dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();

    users.insert(user("a")).unwrap();
    users.insert(user("b")).unwrap();
    users
        .insert(user("c").with("email", "x@example.com"))
        .unwrap();

    assert_eq!(
        code(users.insert(user("d").with("email", "x@example.com"))),
        "DUPLICATE_KEY"
    );

    users
        .put(user("c").with("id", 3).with("email", "y@example.com"))
        .unwrap();
    users
        .insert(user("d").with("email", "x@example.com"))
        .unwrap();
    users
        .put(user("d").with("id", 4).with("email", "x@example.com"))
        .unwrap();

    assert_eq!(
        code(users.put(user("c").with("id", 3).with("email", "x@example.com"))),
        "DUPLICATE_KEY"
    );

    users.delete(4).unwrap();
    users
        .put(user("c").with("id", 3).with("email", "x@example.com"))
        .unwrap();
    txn.commit().unwrap();
}

/// An update sets the fields it names and keeps the rest, and the object is
/// what a put of it would have written: null makes an optional field null
/// and a required one its default, an embedded object or a list is replaced
/// whole, and the indexes follow. A refused update changes nothing.
#[test]
fn an_update_changes_only_the_fields_it_names() {
    let dir = TestDir::new();
    let schema = v1().collection(
        Collection::new("places")
            .primary_key("code", Type::String)
            .field("name", Type::String)
            .optional(
                "address",
                Type::object(
                    darudb::Embedded::new()
                        .field("city", Type::String)
                        .with_default("floor", Type::Int, 1),
                ),
            )
            .optional("tags", Type::list(Type::String))
            .index("tags"),
    );
    let db = open(&dir, schema, &[]).unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();

    users
        .insert(user("Alice").with("email", "a@example.com").with("age", 30))
        .unwrap();
    users
        .insert(user("Bob").with("email", "b@example.com"))
        .unwrap();

    assert!(
        users
            .update(1, Object::new().with("age", 31).with("email", Value::Null))
            .unwrap()
    );
    assert_eq!(
        users.get(1).unwrap(),
        Some(
            user("Alice")
                .with("id", 1)
                .with("email", Value::Null)
                .with("age", 31)
        )
    );
    assert!(
        users
            .update(2, Object::new().with("age", Value::Null))
            .unwrap()
    );
    assert_eq!(
        users.get(2).unwrap().unwrap().get("age"),
        Some(&Value::Int(0))
    );
    assert!(!users.update(9, Object::new().with("age", 1)).unwrap());
    assert!(users.update(1, Object::new().with("id", 1)).unwrap());

    // Refused: another object's unique value, the key, a field the
    // collection does not have, a value of another type, and a required
    // field without a default made null.
    for changes in [
        Object::new().with("email", "b@example.com").with("age", 50),
        Object::new().with("id", 5),
        Object::new().with("nickname", "Al"),
        Object::new().with("age", "old"),
        Object::new().with("name", Value::Null),
    ] {
        let before = users.get(1).unwrap();

        assert_ne!(code(users.update(1, changes.clone())), "OK", "{changes:?}");
        assert_eq!(users.get(1).unwrap(), before, "{changes:?}");
    }

    // The email Alice gave up is free, and the index finds her age.
    users
        .insert(user("Carol").with("email", "a@example.com"))
        .unwrap();
    drop(users);

    let mut places = txn.collection("places").unwrap();
    let address = |city: &str| Object::new().with("city", city);

    places
        .insert(
            Object::new()
                .with("code", "hq")
                .with("name", "Head office")
                .with("address", address("Seoul").with("floor", 9))
                .with("tags", vec![Value::from("main"), Value::from("big")]),
        )
        .unwrap();
    assert!(
        places
            .update(
                "hq",
                Object::new()
                    .with("address", address("Busan"))
                    .with("tags", vec![Value::from("south")])
            )
            .unwrap()
    );
    assert_eq!(
        places.get("hq").unwrap(),
        Some(
            Object::new()
                .with("code", "hq")
                .with("name", "Head office")
                .with("address", address("Busan").with("floor", 1))
                .with("tags", vec![Value::from("south")])
        )
    );
    assert_eq!(
        code(places.update("hq", Object::new().with("code", "hr"))),
        "INVALID_ARGUMENT"
    );
    drop(places);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let count = |collection: &str, filter: darudb::Filter| {
        read.collection(collection)
            .unwrap()
            .count(&darudb::Query::new().filter(filter))
            .unwrap()
    };

    assert_eq!(count("users", darudb::Filter::eq("age", 31)), 1);
    assert_eq!(
        count("users", darudb::Filter::eq("email", "a@example.com")),
        1
    );
    assert_eq!(count("places", darudb::Filter::eq("tags", "south")), 1);
    assert_eq!(count("places", darudb::Filter::eq("tags", "main")), 0);
}

#[test]
fn the_schema_is_checked_every_time_the_file_opens() {
    let dir = TestDir::new();

    with_data(&dir);

    let reordered = Schema::new(1).collection(posts_v1()).collection(users_v1());
    let changed = Schema::new(1).collection(
        Collection::new("users")
            .field("name", Type::String)
            .optional("email", Type::String),
    );

    assert_eq!(code(open(&dir, v1(), &[])), "OK");
    assert_eq!(
        code(open(&dir, reordered, &[])),
        "OK",
        "order does not matter"
    );
    assert_eq!(code(open(&dir, changed, &[])), "SCHEMA_MISMATCH");
    assert_eq!(code(open(&dir, Schema::new(1), &[])), "SCHEMA_MISMATCH");

    let v2 = Schema::new(2).collection(users_v1());
    let dropped_posts = Migration::to(2).delete_collection("posts");

    open(&dir, v2, &[dropped_posts]).unwrap();

    assert_eq!(code(open(&dir, v1(), &[])), "SCHEMA_TOO_NEW");
}

#[test]
fn declarations_that_cannot_be_stored_are_refused_before_a_file_is_made() {
    let dir = TestDir::new();
    let broken = [
        (Schema::new(0), vec![]),
        (
            Schema::new(1).collection(Collection::new("a").field("b", Type::link("c"))),
            vec![],
        ),
        (v1(), vec![Migration::to(2)]),
        (
            Schema::new(3).collection(Collection::new("a")),
            vec![Migration::to(1)],
        ),
    ];

    for (schema, migrations) in broken {
        assert_eq!(code(open(&dir, schema, &migrations)), "INVALID_ARGUMENT");
        assert!(!dir.path("app.darudb").exists());
    }

    assert_eq!(
        code(
            OpenOptions::new()
                .migration(Migration::to(2))
                .open(dir.path("app.darudb"))
        ),
        "INVALID_ARGUMENT"
    );
}

#[test]
fn a_database_without_a_schema_keeps_its_trees_to_itself() {
    let dir = TestDir::new();

    with_data(&dir);

    let db = Database::open(dir.path("app.darudb")).unwrap();
    let mut txn = db.begin_write().unwrap();

    // The file's own schema gives the handle its collections, and the trees
    // that hold them stay the engine's.
    assert_eq!(txn.collection("users").unwrap().len().unwrap(), 2);
    assert_eq!(code(txn.get("\0meta", b"schema")), "INVALID_ARGUMENT");
    assert_eq!(code(txn.insert("\0rec/1", b"k", b"v")), "INVALID_ARGUMENT");

    txn.insert("cache", b"k", b"v").unwrap();
    txn.commit().unwrap();

    assert_eq!(db.begin_read().unwrap().tree_names().unwrap(), ["cache"]);
    assert_eq!(objects(&open(&dir, v1(), &[]).unwrap(), "users").len(), 2);
}

#[test]
fn a_file_opened_without_a_schema_is_read_and_written_with_its_own() {
    let dir = TestDir::new();

    with_data(&dir);

    let db = Database::open(dir.path("app.darudb")).unwrap();
    let schema = db.stored_schema().unwrap();

    assert_eq!(schema.version(), 1);
    assert_eq!(
        schema
            .collections()
            .iter()
            .map(|collection| collection.name())
            .collect::<Vec<_>>(),
        ["users", "posts"]
    );

    let users = &schema.collections()[0];
    let posts = &schema.collections()[1];

    assert_eq!(users.key(), "id");
    assert!(users.is_auto_increment());
    assert_eq!(
        users
            .fields()
            .iter()
            .map(|field| (field.name(), field.kind().clone(), field.is_optional()))
            .collect::<Vec<_>>(),
        [
            ("name", Type::String, false),
            ("email", Type::String, true),
            ("age", Type::Int, false),
        ]
    );
    assert_eq!(users.fields()[2].default(), Some(&Value::Int(0)));
    assert_eq!(users.fields()[0].default(), None);
    assert_eq!(
        users.indexes().collect::<Vec<_>>(),
        [("email", true), ("age", false)]
    );
    assert_eq!(posts.key(), "slug");
    assert!(!posts.is_auto_increment());
    assert_eq!(posts.fields()[1].kind(), &Type::link("users"));
    assert_eq!(posts.fields()[2].kind(), &Type::list(Type::String));
    assert_eq!(
        posts.indexes().collect::<Vec<_>>(),
        [("author", false), ("tags", false)]
    );
    // The record is the one a handle that declares the schema gets.
    assert_eq!(
        db.schema_record(),
        open(&dir, v1(), &[]).unwrap().schema_record()
    );

    // Objects read as they were written, and the stored schema's checks and
    // indexes hold for what is written now.
    assert_eq!(
        objects(&db, "users")
            .iter()
            .map(|object| object.get("name").cloned())
            .collect::<Vec<_>>(),
        [Some(Value::from("Alice")), Some(Value::from("Bob"))]
    );

    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();

    assert_eq!(
        users.insert(user("Carol").with("age", 41)).unwrap(),
        3.into()
    );
    assert_eq!(
        code(users.insert(user("Mallory").with("email", "alice@example.com"))),
        "DUPLICATE_KEY"
    );
    assert_eq!(code(users.insert(Object::new())), "INVALID_ARGUMENT");
    assert!(users.update(2, Object::new().with("age", 7)).unwrap());
    assert_eq!(
        users
            .count(&darudb::Query::new().filter(darudb::Filter::ge("age", 7)))
            .unwrap(),
        3
    );
    txn.commit().unwrap();

    // A handle that declares the schema reads what was written without it.
    let declared = open(&dir, v1(), &[]).unwrap();
    let ages = objects(&declared, "users")
        .iter()
        .map(|object| object.get("age").cloned())
        .collect::<Vec<_>>();

    assert_eq!(
        ages,
        [
            Some(Value::Int(30)),
            Some(Value::Int(7)),
            Some(Value::Int(41))
        ]
    );
}

#[test]
fn a_file_that_holds_no_schema_gives_a_handle_without_one_no_collections() {
    let dir = TestDir::new();
    let db = Database::open(dir.path("plain.darudb")).unwrap();

    assert!(db.stored_schema().is_none());
    assert!(db.schema_record().is_none());

    let error = db.begin_read().unwrap().collection("users").err().unwrap();

    assert_eq!(error.code(), "INVALID_ARGUMENT");
    assert!(error.to_string().contains("holds none"), "{error}");

    // A schema stored after the handle opened the file is not its own: it
    // keeps the schema it opened with, which was none.
    open_at(&dir.path("plain.darudb"), v1()).unwrap();

    let error = db.begin_read().unwrap().collection("users").err().unwrap();

    assert_eq!(error.code(), "INVALID_ARGUMENT");
    assert!(error.to_string().contains("open it again"), "{error}");
    assert_eq!(
        Database::open(dir.path("plain.darudb"))
            .unwrap()
            .stored_schema(),
        Some(v1())
    );
}

#[test]
fn a_handle_without_a_schema_notices_that_the_file_was_migrated_under_it() {
    let dir = TestDir::new();

    with_data(&dir);

    let own = Database::open(dir.path("app.darudb")).unwrap();
    let v2 = Schema::new(2)
        .collection(users_v1().optional("nickname", Type::String))
        .collection(posts_v1());

    open(&dir, v2.clone(), &[]).unwrap();

    assert_eq!(
        code(own.begin_read().unwrap().collection("users")),
        "SCHEMA_MISMATCH"
    );
    // Its description stays the schema it opened with.
    assert_eq!(own.stored_schema().unwrap().version(), 1);
    assert_eq!(
        Database::open(dir.path("app.darudb"))
            .unwrap()
            .stored_schema(),
        Some(v2)
    );
}

#[test]
fn an_encrypted_file_opened_without_a_schema_needs_its_key_as_ever() {
    let dir = TestDir::new();
    let path = dir.path("secret.darudb");
    let mut options = OpenOptions::new();

    options.schema(v1()).password("correct horse");
    options
        .open(&path)
        .unwrap()
        .begin_write()
        .and_then(|mut txn| {
            txn.collection("users")?.insert(user("Alice"))?;
            txn.commit()
        })
        .unwrap();

    assert_eq!(code(Database::open(&path)), "KEY_REQUIRED");

    let db = OpenOptions::new()
        .password("correct horse")
        .open(&path)
        .unwrap();

    assert_eq!(db.stored_schema(), Some(v1()));
    assert_eq!(objects(&db, "users").len(), 1);
}

#[test]
fn a_migration_renames_without_rewriting_and_adds_what_is_new() {
    let dir = TestDir::new();

    with_data(&dir);

    let v2 = Schema::new(2)
        .collection(
            Collection::new("people")
                .field("full_name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .optional("nickname", Type::String)
                .with_default("active", Type::Bool, true)
                .unique("email")
                .index("full_name"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("people"))
                .optional("tags", Type::list(Type::String))
                .index("author"),
        );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name");
    let db = open(&dir, v2.clone(), std::slice::from_ref(&migration)).unwrap();

    assert_eq!(
        objects(&db, "people"),
        [
            Object::new()
                .with("id", 1)
                .with("full_name", "Alice")
                .with("email", "alice@example.com")
                .with("age", 30)
                .with("nickname", Value::Null)
                .with("active", true),
            Object::new()
                .with("id", 2)
                .with("full_name", "Bob")
                .with("email", Value::Null)
                .with("age", 0)
                .with("nickname", Value::Null)
                .with("active", true),
        ]
    );

    // The unique index came through the rename.
    let mut txn = db.begin_write().unwrap();

    assert_eq!(
        code(
            txn.collection("people").unwrap().insert(
                Object::new()
                    .with("full_name", "Eve")
                    .with("email", "alice@example.com")
            )
        ),
        "DUPLICATE_KEY"
    );
    assert_eq!(code(txn.collection("users")), "INVALID_ARGUMENT");
    drop(txn);
    drop(db);

    // Opening again finds version 2 stored and has nothing to do.
    assert_eq!(code(open(&dir, v2, &[migration])), "OK");
}

#[test]
fn a_migration_function_moves_data_with_the_previous_schema() {
    let dir = TestDir::new();

    with_data(&dir);

    let v2 = Schema::new(2)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::String, "")
                .optional("post_count", Type::Int)
                .unique("email")
                .index("age"),
        )
        .collection(
            Collection::new("articles")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users")),
        );
    let migration = Migration::to(2)
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            assert_eq!(migrating.previous_version(), 1);

            for key in migrating.previous_keys("users")? {
                let old = migrating.previous("users", key.clone())?.unwrap();
                let age = old.get("age").and_then(Value::as_int).unwrap_or(0);
                let mut users = migrating.collection("users")?;
                let mut user = users.get(key)?.unwrap();

                user.set("age", format!("{age} years"));
                users.put(user)?;
            }

            for key in migrating.previous_keys("posts")? {
                let post = migrating.previous("posts", key)?.unwrap();
                let author = post.get("author").cloned().unwrap();
                let mut articles = migrating.collection("articles")?;

                articles.insert(
                    Object::new()
                        .with("slug", post.get("slug").cloned().unwrap())
                        .with("author", author.clone()),
                )?;

                let mut users = migrating.collection("users")?;
                let mut user = users.get(author)?.unwrap();

                user.set("post_count", 1);
                users.put(user)?;
            }

            Ok(())
        });
    let db = open(&dir, v2, &[migration]).unwrap();
    let users = objects(&db, "users");

    assert_eq!(users[0].get("age"), Some(&Value::from("30 years")));
    assert_eq!(users[1].get("age"), Some(&Value::from("0 years")));
    assert_eq!(users[0].get("post_count"), Some(&Value::Int(1)));
    assert_eq!(users[1].get("post_count"), Some(&Value::Null));
    assert_eq!(
        objects(&db, "articles"),
        [Object::new().with("slug", "hello").with("author", 1)]
    );
    assert_eq!(
        code(db.begin_read().unwrap().collection("posts")),
        "INVALID_ARGUMENT"
    );
}

#[test]
fn a_failed_migration_leaves_the_file_as_it_was() {
    let dir = TestDir::new();

    with_data(&dir);

    let v2 = Schema::new(2)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .index("age"),
        )
        .collection(Collection::new("notes").field("text", Type::String));
    let failing = Migration::to(2)
        .delete_collection("posts")
        .run(|migrating| {
            migrating.collection("users")?.delete(1)?;
            migrating
                .collection("notes")?
                .insert(Object::new().with("text", "hi"))?;

            Err(Error::MigrationFailed {
                message: "not today".to_owned(),
            })
        });

    assert_eq!(code(open(&dir, v2.clone(), &[failing])), "MIGRATION_FAILED");

    // A unique index built over values that repeat fails the same way.
    let unique_age = Schema::new(2)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email")
                .unique("name")
                .index("age"),
        )
        .collection(posts_v1());
    let db = open(&dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("users")
        .unwrap()
        .insert(user("Bob"))
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    assert_eq!(code(open(&dir, unique_age, &[])), "DUPLICATE_KEY");

    let db = open(&dir, v1(), &[]).unwrap();

    assert_eq!(objects(&db, "users").len(), 3);
    assert_eq!(objects(&db, "posts").len(), 1);
}

#[test]
fn a_handle_notices_that_the_file_was_migrated_under_it() {
    let dir = TestDir::new();

    with_data(&dir);

    let old = open(&dir, v1(), &[]).unwrap();
    let read = old.begin_read().unwrap();
    let v2 = Schema::new(2)
        .collection(users_v1().optional("nickname", Type::String))
        .collection(posts_v1());
    let new = open(&dir, v2, &[]).unwrap();

    // A read transaction that began before the migration still sees the
    // schema it began with.
    assert_eq!(read.collection("users").unwrap().len().unwrap(), 2);
    assert_eq!(
        code(old.begin_read().unwrap().collection("users")),
        "SCHEMA_MISMATCH"
    );
    assert_eq!(
        code(old.begin_write().unwrap().collection("users")),
        "SCHEMA_MISMATCH"
    );
    assert_eq!(objects(&new, "users").len(), 2);
}

#[test]
fn an_encrypted_database_holds_objects_too() {
    let dir = TestDir::new();
    let path = dir.path("app.darudb");
    let db = OpenOptions::new()
        .key([3; 32])
        .schema(v1())
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("users")
        .unwrap()
        .insert(user("Alice"))
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    assert_eq!(
        code(OpenOptions::new().schema(v1()).open(&path)),
        "KEY_REQUIRED"
    );

    let db = OpenOptions::new()
        .key([3; 32])
        .schema(v1())
        .open(&path)
        .unwrap();

    assert_eq!(
        objects(&db, "users")[0].get("name"),
        Some(&Value::from("Alice"))
    );
}

#[test]
fn records_and_the_ir_carry_objects_and_queries_across_the_language_boundary() {
    use darudb::{Filter, Query, QueryRequest};

    let dir = TestDir::new();

    with_data(&dir);

    let db = open(&dir, v1(), &[]).unwrap();

    // The stored schema's record reads back as the schema it was declared as.
    let declared = Schema::decode(db.schema_record().unwrap()).unwrap();

    assert_eq!(declared, v1());
    assert_eq!(
        code(Schema::decode(&[0xFF, 1, 2])),
        "INVALID_ARGUMENT",
        "a record that does not decode"
    );

    // A record as the file holds it writes back as the same object.
    let mut txn = db.begin_write().unwrap();
    let mut users = txn.collection("users").unwrap();
    let alice = users.get(1).unwrap().unwrap();
    let record = users.get_record(1).unwrap().unwrap();

    // The same record lent, with the transaction's changes, and nothing for
    // a key no object has; an error the callback returns is returned.
    let mut lent = Vec::new();

    assert!(
        users
            .get_record_with(1, |record| {
                lent.extend_from_slice(record);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(lent, record);
    assert!(
        !users
            .get_record_with(99, |_| panic!("no object has the key 99"))
            .unwrap()
    );
    assert_eq!(
        code(
            users.get_record_with(1, |_| Err(darudb::Error::InvalidArgument {
                message: "stop".to_owned(),
            }))
        ),
        "INVALID_ARGUMENT"
    );

    assert!(users.delete(1).unwrap());
    assert_eq!(users.insert_record(&record).unwrap(), Value::Int(1));
    assert_eq!(users.get(1).unwrap(), Some(alice.clone()));
    assert_eq!(code(users.insert_record(&record)), "DUPLICATE_KEY");
    assert_eq!(users.put_record(&record).unwrap(), Value::Int(1));
    assert_eq!(code(users.insert_record(&[0xFF])), "INVALID_ARGUMENT");
    // One field, id 99, which `users` does not have.
    assert_eq!(
        code(users.insert_record(&[1, 99, 0x02])),
        "INVALID_ARGUMENT"
    );
    // One field, the name, holding an int.
    assert_eq!(
        code(users.insert_record(&[1, 2, 0x04, 2])),
        "INVALID_ARGUMENT"
    );
    txn.commit().unwrap();

    // A query crosses as IR and returns the objects' records.
    let query = Query::new()
        .filter(Filter::ge("age", 0))
        .sort_by_desc("age");
    let request = QueryRequest::new("users", query.clone());
    let decoded = QueryRequest::decode(&request.encode().unwrap()).unwrap();

    assert_eq!(decoded, request);

    // An aggregate crosses with its function and the path of its field.
    let mut summing = QueryRequest::new("users", query.clone());

    summing.aggregate = Some((darudb::Aggregate::Sum, vec!["age".to_owned()]));

    let summed = QueryRequest::decode(&summing.encode().unwrap()).unwrap();

    assert_eq!(summed, summing);
    summing.count = true;
    assert_eq!(code(summing.encode()), "INVALID_QUERY");

    let read = db.begin_read().unwrap();
    let users = read.collection("users").unwrap();
    let records = users.query_records(&decoded.query).unwrap();
    let expected: Vec<Vec<u8>> = users
        .query(&query)
        .unwrap()
        .iter()
        .map(|user| {
            users
                .get_record(user.get("id").cloned().unwrap())
                .unwrap()
                .unwrap()
        })
        .collect();

    assert_eq!(records, expected);
    assert_eq!(records.len(), 2);

    let mut lent = None;

    assert!(
        users
            .get_record_with(1, |record| {
                lent = Some(record.to_vec());
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(lent, users.get_record(1).unwrap());

    // The same records, lent one at a time, and the first error stops them.
    let mut lent = Vec::new();

    users
        .query_records_with(&decoded.query, |record| {
            lent.push(record.to_vec());
            Ok(())
        })
        .unwrap();
    assert_eq!(lent, expected);

    let mut calls = 0;
    let stopped = users.query_records_with(&decoded.query, |_| {
        calls += 1;
        Err(darudb::Error::InvalidArgument {
            message: "stop".to_owned(),
        })
    });

    assert_eq!(code(stopped), "INVALID_ARGUMENT");
    assert_eq!(calls, 1);
    assert_eq!(code(QueryRequest::decode(&[0xFF])), "INVALID_QUERY");
}

#[test]
fn a_migration_can_stop_for_its_caller_between_its_steps() {
    use darudb::Opening;

    let dir = TestDir::new();

    with_data(&dir);

    let v3 = Schema::new(3)
        .collection(
            users_v1()
                .optional("nickname", Type::String)
                .optional("label", Type::String),
        )
        .collection(posts_v1());
    let mut options = OpenOptions::new();

    options
        .schema(v3)
        .migration(Migration::to(2).run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let mut users = migrating.collection("users")?;
                let mut user = users.get(key.clone())?.unwrap();

                user.set("nickname", format!("n{}", key.as_int().unwrap()));
                users.put(user)?;
            }

            Ok(())
        }));

    // Dropped halfway, the migration leaves the file at version 1.
    let Opening::Migrating(mut pending) = options.open_migrating(dir.path("app.darudb")).unwrap()
    else {
        panic!("the file holds version 1");
    };

    assert_eq!((pending.previous_version(), pending.version()), (1, 3));
    assert_eq!(
        Schema::decode(&pending.previous_schema_record()).unwrap(),
        v1()
    );
    assert_eq!(pending.next_step().unwrap(), Some(2));
    assert!(
        pending
            .migrating()
            .previous_record("users", 1)
            .unwrap()
            .is_some()
    );
    assert!(
        pending
            .migrating()
            .previous_record("users", 9)
            .unwrap()
            .is_none()
    );
    drop(pending);
    assert_eq!(objects(&open(&dir, v1(), &[]).unwrap(), "users").len(), 2);

    let Opening::Migrating(mut pending) = options.open_migrating(dir.path("app.darudb")).unwrap()
    else {
        panic!("the file holds version 1");
    };
    let mut labelled = Vec::new();

    // Each step runs its own function first, then returns to the caller,
    // which runs one of its own in the same transaction.
    while let Some(version) = pending.next_step().unwrap() {
        let mut migrating = pending.migrating();

        for key in migrating.previous_keys("users").unwrap() {
            let mut users = migrating.collection("users").unwrap();
            let mut user = users.get(key).unwrap().unwrap();
            let nickname = user
                .get("nickname")
                .and_then(Value::as_str)
                .unwrap_or("none");

            user.set("label", format!("{nickname} at {version}"));
            labelled.push(version);
            users.put(user).unwrap();
        }
    }

    let db = pending.finish().unwrap();

    assert_eq!(labelled, [2, 2, 3, 3]);
    assert_eq!(
        objects(&db, "users")
            .iter()
            .map(|user| user
                .get("label")
                .and_then(Value::as_str)
                .unwrap()
                .to_owned())
            .collect::<Vec<_>>(),
        ["n1 at 3", "n2 at 3"]
    );
    assert!(matches!(
        options.open_migrating(dir.path("app.darudb")).unwrap(),
        Opening::Open(_)
    ));
}

/// What a language binding reads into a type it declares at run time: each
/// field of a record by its slot, as the engine's `Object` of the same
/// record holds it, with defaults and nulls for the fields a record leaves
/// out, and an embedded object, a list and a link read through the value.
/// A shape whose fields do not match the collection's is refused.
#[test]
fn a_shape_reads_records_as_objects_hold_them() {
    use darudb::__binding::{Scalar, ShapeField};

    let dir = TestDir::new();
    let schema = v1().collection(
        Collection::new("places")
            .primary_key("code", Type::String)
            .field("name", Type::String)
            .field("owner", Type::link("users"))
            .optional(
                "address",
                Type::object(
                    darudb::Embedded::new()
                        .field("city", Type::String)
                        .with_default("floor", Type::Int, 1),
                ),
            )
            .optional("tags", Type::list(Type::String)),
    );
    let db = open(&dir, schema, &[]).unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("users")
        .unwrap()
        .insert(user("Alice"))
        .unwrap();

    let mut places = txn.collection("places").unwrap();

    places
        .insert(
            Object::new()
                .with("code", "a")
                .with("name", "Hall")
                .with("owner", 1)
                .with("address", Object::new().with("city", "Seoul"))
                .with("tags", Value::List(vec!["old".into(), "big".into()])),
        )
        .unwrap();
    places
        .insert(
            Object::new()
                .with("code", "b")
                .with("name", "Shed")
                .with("owner", 1),
        )
        .unwrap();
    drop(places);
    txn.commit().unwrap();

    // The class's fields in an order of its own, as a binding's class has
    // them.
    let field = |name: &str| ShapeField {
        name: name.to_owned(),
        fields: None,
    };
    let fields = || {
        vec![
            field("tags"),
            field("name"),
            ShapeField {
                name: "address".to_owned(),
                fields: Some(vec![field("floor"), field("city")]),
            },
            field("code"),
            field("owner"),
        ]
    };
    let read = db.begin_read().unwrap();
    let places = read.collection("places").unwrap();
    let shape = places.shape(1, fields).unwrap();
    let mut found = Vec::new();

    places
        .query_records_with(&darudb::Query::new(), |record| {
            shape.read(record, |mut fields| {
                let mut object = Object::new();

                while let Some((slot, value)) = fields.next()? {
                    let read = match slot {
                        0 if value.is_null() => Value::Null,
                        0 => Value::List(
                            value
                                .elements()?
                                .into_iter()
                                .map(|element| element.str().map(Value::from))
                                .collect::<darudb::Result<_>>()?,
                        ),
                        1 | 3 => value.str()?.into(),
                        2 if value.is_null() => Value::Null,
                        2 => {
                            let mut address = Object::new();
                            let mut inner = value.fields()?;

                            while let Some((slot, value)) = inner.next()? {
                                match slot {
                                    0 => address.set("floor", value.int()?),
                                    _ => address.set("city", value.str()?),
                                };
                            }

                            Value::Object(address)
                        }
                        _ => match value.linked()?.scalar()? {
                            Scalar::Int(key) => key.into(),
                            other => panic!("a link to users holds {other:?}"),
                        },
                    };
                    let name = ["tags", "name", "address", "code", "owner"][slot];

                    object.set(name, read);
                }

                found.push(object);
                Ok(())
            })
        })
        .unwrap();

    let objects: Vec<Object> = places
        .query(&darudb::Query::new())
        .unwrap()
        .into_iter()
        .map(|mut object| {
            // `Object` leaves a missing optional field out where the shape
            // gives it as null.
            for name in ["address", "tags"] {
                if object.get(name).is_none() {
                    object.set(name, Value::Null);
                }
            }

            object
        })
        .collect();

    assert_eq!(found, objects);
    assert_eq!(
        found[0].get("address"),
        Some(&Value::Object(
            Object::new().with("city", "Seoul").with("floor", 1)
        ))
    );

    // A field the collection lacks, one the class lacks, and an embedded
    // object read as a scalar.
    for (key, fields) in [
        (
            2,
            vec![
                field("name"),
                field("code"),
                field("owner"),
                field("tags"),
                field("address"),
                field("rooms"),
            ],
        ),
        (
            3,
            vec![field("name"), field("code"), field("owner"), field("tags")],
        ),
        (
            4,
            vec![
                field("name"),
                field("code"),
                field("owner"),
                field("tags"),
                field("address"),
            ],
        ),
    ] {
        assert_eq!(
            places.shape(key, || fields).unwrap_err().code(),
            "INVALID_ARGUMENT"
        );
    }
}

/// A binding writes an object through a shape as `insert` and `put` write
/// the `Object` it would have made: an auto-increment number for a key left
/// out, a field's default, its index entries, and the same refusals.
#[test]
fn a_shape_writes_objects_as_insert_does() {
    use darudb::__binding::ShapeField;
    use darudb::ValueWriter;

    /// A field's value, as a binding holds it before writing it.
    #[derive(Clone)]
    enum Held {
        Null,
        Int(i64),
        Str(&'static str),
        Link(i64),
        Tags(Vec<String>),
    }

    fn write(value: &Held, writer: ValueWriter<'_>) -> darudb::Result<()> {
        match value {
            Held::Null => writer.null(),
            Held::Int(value) => writer.int(*value),
            Held::Str(value) => writer.string(value),
            Held::Link(key) => writer.link(key),
            Held::Tags(tags) => writer.list(tags),
        }
    }

    let field = |name: &str| ShapeField {
        name: name.to_owned(),
        fields: None,
    };
    // In an order of the class's own.
    let users = || vec![field("age"), field("id"), field("name"), field("email")];
    let posts = || vec![field("tags"), field("author"), field("slug")];
    let dir = TestDir::new();
    let db = open(&dir, v1(), &[]).unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut writer = txn.collection("users").unwrap();
    let mut user = |values: [Held; 4], replace: bool| {
        writer.write_shaped(1, users, replace, |slot, value| write(&values[slot], value))
    };

    assert_eq!(
        user(
            [
                Held::Int(30),
                Held::Null,
                Held::Str("Alice"),
                Held::Str("a@x")
            ],
            false
        )
        .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        user(
            [Held::Null, Held::Null, Held::Str("Bob"), Held::Null],
            false
        )
        .unwrap(),
        Value::Int(2)
    );
    assert_eq!(
        user(
            [
                Held::Int(41),
                Held::Int(2),
                Held::Str("Bea"),
                Held::Str("b@x")
            ],
            true
        )
        .unwrap(),
        Value::Int(2)
    );
    // A unique value taken, a key taken, a value of another type, and a
    // required field left out.
    for (values, refused) in [
        (
            [Held::Null, Held::Null, Held::Str("Cy"), Held::Str("a@x")],
            "DUPLICATE_KEY",
        ),
        (
            [Held::Null, Held::Int(1), Held::Str("Cy"), Held::Null],
            "DUPLICATE_KEY",
        ),
        (
            [Held::Str("old"), Held::Null, Held::Str("Cy"), Held::Null],
            "INVALID_ARGUMENT",
        ),
        (
            [Held::Int(5), Held::Null, Held::Null, Held::Null],
            "INVALID_ARGUMENT",
        ),
    ] {
        assert_eq!(code(user(values, false)), refused);
    }

    assert_eq!(
        user(
            [Held::Int(5), Held::Null, Held::Str("Cy"), Held::Null],
            false
        )
        .unwrap(),
        Value::Int(3)
    );

    // A collection with a link and a list, whose record the engine reads
    // into an object to check it.
    let mut writer = txn.collection("posts").unwrap();

    writer
        .write_shaped(1, posts, false, |slot, value| {
            write(
                &[
                    Held::Tags(vec!["new".to_owned()]),
                    Held::Link(2),
                    Held::Str("hello"),
                ][slot],
                value,
            )
        })
        .unwrap();
    assert_eq!(
        code(writer.write_shaped(1, posts, false, |slot, value| {
            write(
                &[Held::Null, Held::Str("Bea"), Held::Str("named")][slot],
                value,
            )
        })),
        "INVALID_ARGUMENT"
    );
    drop(writer);
    txn.commit().unwrap();

    assert_eq!(
        objects(&db, "users"),
        [
            Object::new()
                .with("id", 1)
                .with("name", "Alice")
                .with("email", "a@x")
                .with("age", 30),
            Object::new()
                .with("id", 2)
                .with("name", "Bea")
                .with("email", "b@x")
                .with("age", 41),
            Object::new()
                .with("id", 3)
                .with("name", "Cy")
                .with("email", Value::Null)
                .with("age", 5),
        ]
    );
    assert_eq!(
        objects(&db, "posts"),
        [Object::new()
            .with("slug", "hello")
            .with("author", 2)
            .with("tags", Value::List(vec!["new".into()]))]
    );

    let read = db.begin_read().unwrap();
    let found = read
        .collection("users")
        .unwrap()
        .query(&darudb::Query::new().filter(darudb::Filter::ge("age", 30)))
        .unwrap();

    assert_eq!(found.len(), 2);
}
