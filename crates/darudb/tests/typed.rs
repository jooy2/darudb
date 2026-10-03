//! Objects as Rust types through the public API: `#[derive(Object)]` and
//! `#[derive(Embedded)]`, typed reads and writes, and their agreement with
//! the untyped `Object` API on one file.
//!
//! The derives come from `darudb-derive` directly, a development dependency,
//! named by their path so that these tests run with the `derive` feature and
//! without it: with it, `darudb::Object` is the derive as well as the type.

// The helpers below fail the test that called them by panicking, the way a
// test does; `clippy.toml` allows that inside tests but not in helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::TestDir;
use darudb::{
    Collection, Database, Filter, Link, Migration, Object, OpenOptions, Query, Schema, Type, Value,
};

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "people")]
struct Person {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
    score: f64,
    active: bool,
    tags: Vec<String>,
    #[darudb(rename = "photo")]
    picture: Option<Vec<u8>>,
    home: Option<Address>,
    friend: Option<Link<Person>>,
}

#[derive(darudb_derive::Embedded, Debug, Clone, PartialEq)]
struct Address {
    city: String,
    #[darudb(rename = "zip")]
    postal_code: Option<String>,
    point: Point,
}

#[derive(darudb_derive::Embedded, Debug, Clone, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "posts")]
struct Post {
    #[darudb(key)]
    slug: String,
    #[darudb(index)]
    author: Link<Person>,
    readers: Vec<Link<Person>>,
    scores: Vec<i64>,
}

fn schema() -> Schema {
    Schema::new(1)
        .collection(Collection::of::<Person>())
        .collection(Collection::of::<Post>())
}

fn open(dir: &TestDir, schema: Schema) -> Database {
    OpenOptions::new()
        .schema(schema)
        .open(dir.path("app.darudb"))
        .unwrap()
}

fn code<T>(result: darudb::Result<T>) -> &'static str {
    result.err().map_or("OK", |error| error.code())
}

fn ada() -> Person {
    Person {
        id: None,
        name: "Ada".to_owned(),
        email: Some("ada@example.com".to_owned()),
        age: 36,
        score: 0.5,
        active: true,
        tags: vec!["math".to_owned(), "engines".to_owned()],
        picture: Some(vec![0, 1, 2, 255]),
        home: Some(Address {
            city: "London".to_owned(),
            postal_code: None,
            point: Point { x: -0.12, y: 51.5 },
        }),
        friend: None,
    }
}

fn grace() -> Person {
    Person {
        id: None,
        name: "Grace".to_owned(),
        email: None,
        age: 85,
        score: -1.25,
        active: false,
        tags: Vec::new(),
        picture: None,
        home: None,
        friend: Some(Link::new(1)),
    }
}

#[test]
fn the_derived_collection_is_the_one_a_builder_declares() {
    let built = Collection::new("people")
        .field("name", Type::String)
        .optional("email", Type::String)
        .with_default("age", Type::Int, 0)
        .field("score", Type::Float)
        .field("active", Type::Bool)
        .field("tags", Type::list(Type::String))
        .optional("photo", Type::Bytes)
        .optional(
            "home",
            Type::object(
                darudb::Embedded::new()
                    .field("city", Type::String)
                    .optional("zip", Type::String)
                    .field(
                        "point",
                        Type::object(
                            darudb::Embedded::new()
                                .field("x", Type::Float)
                                .field("y", Type::Float),
                        ),
                    ),
            ),
        )
        .optional("friend", Type::link("people"))
        .unique("email")
        .index("age");

    assert_eq!(Collection::of::<Person>(), built);
    assert_eq!(
        Collection::of::<Post>(),
        Collection::new("posts")
            .primary_key("slug", Type::String)
            .field("author", Type::link("people"))
            .field("readers", Type::list(Type::link("people")))
            .field("scores", Type::list(Type::Int))
            .index("author")
    );
}

#[test]
fn objects_read_back_as_they_were_written() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();
    let first = people.insert(&ada()).unwrap();
    let second = people.insert(&grace()).unwrap();

    assert_eq!((first, second), (1, 2));
    // The transaction reads its own writes.
    assert_eq!(
        people.get(first).unwrap(),
        Some(Person {
            id: Some(1),
            ..ada()
        })
    );
    drop(people);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let people = read.collection_of::<Person>().unwrap();

    assert_eq!(
        people.get(1).unwrap(),
        Some(Person {
            id: Some(1),
            ..ada()
        })
    );
    assert_eq!(
        people.get(2).unwrap(),
        Some(Person {
            id: Some(2),
            ..grace()
        })
    );
    assert_eq!(people.get(3).unwrap(), None);
    assert_eq!(people.len().unwrap(), 2);
    assert_eq!(
        people
            .iter()
            .unwrap()
            .map(|person| person.unwrap().name)
            .collect::<Vec<_>>(),
        ["Ada", "Grace"]
    );
}

#[test]
fn queries_return_the_objects_the_untyped_api_finds() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();

    for n in 0..50 {
        people
            .insert(&Person {
                name: format!("person {n}"),
                email: Some(format!("{n}@example.com")),
                age: n % 7,
                ..grace()
            })
            .unwrap();
    }

    drop(people);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let typed = read.collection_of::<Person>().unwrap();
    let untyped = read.collection("people").unwrap();
    let queries = [
        Query::new().filter(Filter::eq("age", 3)),
        Query::new().filter(Filter::eq("email", "17@example.com")),
        Query::new()
            .filter(Filter::between("age", 2, 4))
            .sort_by_desc("age")
            .limit(5),
        Query::new().sort_by("name").offset(10).limit(3),
        Query::parse("age >= $0 AND name ENDSWITH \"9\"", &[Value::Int(5)]).unwrap(),
    ];

    for query in &queries {
        let found = typed.query(query).unwrap();
        let expected = untyped.query(query).unwrap();

        assert_eq!(
            found.iter().map(|person| person.id).collect::<Vec<_>>(),
            expected
                .iter()
                .map(|object| object.get("id").and_then(Value::as_int))
                .collect::<Vec<_>>(),
            "{query:?}"
        );
        assert_eq!(typed.count(query).unwrap(), untyped.count(query).unwrap());
    }
}

#[test]
fn a_typed_write_reads_back_untyped_and_the_other_way() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();

    txn.collection_of::<Person>()
        .unwrap()
        .insert(&ada())
        .unwrap();
    txn.collection("people")
        .unwrap()
        .insert(
            Object::new()
                .with("name", "Linus")
                .with("score", 2.0)
                .with("active", true)
                .with("tags", vec![Value::from("kernels")])
                .with(
                    "home",
                    Object::new()
                        .with("city", "Helsinki")
                        .with("point", Object::new().with("x", 25.0).with("y", 60.2)),
                ),
        )
        .unwrap();
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let untyped = read.collection("people").unwrap().get(1).unwrap().unwrap();

    assert_eq!(untyped.get("name"), Some(&Value::from("Ada")));
    assert_eq!(
        untyped.get("photo"),
        Some(&Value::from(vec![0u8, 1, 2, 255]))
    );
    assert_eq!(
        untyped
            .get("home")
            .and_then(Value::as_object)
            .and_then(|home| home.get("city")),
        Some(&Value::from("London"))
    );

    let linus = read
        .collection_of::<Person>()
        .unwrap()
        .get(2)
        .unwrap()
        .unwrap();

    assert_eq!(
        linus,
        Person {
            id: Some(2),
            name: "Linus".to_owned(),
            email: None,
            // Left out, so the default.
            age: 0,
            score: 2.0,
            active: true,
            tags: vec!["kernels".to_owned()],
            picture: None,
            home: Some(Address {
                city: "Helsinki".to_owned(),
                postal_code: None,
                point: Point { x: 25.0, y: 60.2 },
            }),
            friend: None,
        }
    );
}

#[test]
fn put_replaces_delete_removes_and_update_sets_fields() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();
    let id = people.insert(&ada()).unwrap();
    let mut changed = people.get(id).unwrap().unwrap();

    changed.age = 37;
    changed.tags.push("poetry".to_owned());
    assert_eq!(people.put(&changed).unwrap(), id);
    assert_eq!(people.get(id).unwrap(), Some(changed.clone()));
    assert!(people.update(id, Object::new().with("age", 38)).unwrap());
    assert_eq!(people.get(id).unwrap().unwrap().age, 38);
    // The age index followed both changes.
    assert_eq!(
        people
            .query(&Query::new().filter(Filter::eq("age", 38)))
            .unwrap()
            .len(),
        1
    );
    assert!(people.delete(id).unwrap());
    assert!(!people.delete(id).unwrap());
    assert_eq!(people.get(id).unwrap(), None);
    assert!(people.is_empty().unwrap());
}

#[test]
fn a_refused_write_leaves_the_transaction_as_it_was() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();

    people.insert(&ada()).unwrap();
    assert_eq!(code(people.insert(&ada())), "DUPLICATE_KEY");
    assert_eq!(
        code(people.insert(&Person {
            id: Some(1),
            ..grace()
        })),
        "DUPLICATE_KEY"
    );
    assert_eq!(people.len().unwrap(), 1);
    drop(people);
    txn.commit().unwrap();
}

#[test]
fn a_declared_key_and_links_round_trip() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();

    txn.collection_of::<Person>()
        .unwrap()
        .insert(&ada())
        .unwrap();
    txn.collection_of::<Person>()
        .unwrap()
        .insert(&grace())
        .unwrap();

    let post = Post {
        slug: "engines".to_owned(),
        author: Link::new(1),
        readers: vec![Link::new(2), Link::new(1), Link::new(99)],
        scores: vec![-3, 0, i64::MAX, i64::MIN],
    };
    let mut posts = txn.collection_of::<Post>().unwrap();

    assert_eq!(posts.insert(&post).unwrap(), "engines");
    assert_eq!(code(posts.insert(&post)), "DUPLICATE_KEY");
    drop(posts);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let posts = read.collection_of::<Post>().unwrap();

    assert_eq!(posts.get("engines").unwrap(), Some(post));
    assert_eq!(
        posts
            .query(&Query::new().filter(Filter::eq("author.name", "Ada")))
            .unwrap()
            .len(),
        1
    );
    // Grace links to Ada, by the key the link holds.
    assert_eq!(
        read.collection_of::<Person>()
            .unwrap()
            .get(2)
            .unwrap()
            .and_then(|grace| grace.friend)
            .map(Link::into_key),
        Some(1)
    );
}

#[derive(darudb_derive::Object, Debug, PartialEq)]
#[darudb(collection = "people")]
struct Stranger {
    id: Option<i64>,
    name: String,
}

#[derive(darudb_derive::Object, Debug, PartialEq)]
#[darudb(collection = "people")]
struct Keyed {
    #[darudb(key)]
    name: String,
}

#[test]
fn a_type_that_does_not_match_its_collection_is_refused() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let read = db.begin_read().unwrap();
    let mismatch = read.collection_of::<Stranger>().err().unwrap();

    assert_eq!(mismatch.code(), "INVALID_ARGUMENT");
    assert!(mismatch.to_string().contains("Stranger"), "{mismatch}");
    assert_eq!(code(read.collection_of::<Keyed>()), "INVALID_ARGUMENT");

    // A database opened without that collection.
    let other = TestDir::new();
    let db = open(
        &other,
        Schema::new(1).collection(Collection::of::<Stranger>()),
    );
    let read = db.begin_read().unwrap();

    assert_eq!(code(read.collection_of::<Person>()), "INVALID_ARGUMENT");
    assert_eq!(code(read.collection_of::<Stranger>()), "OK");
}

mod v1 {
    /// The collection before a migration added `age` and `nickname` and
    /// removed `legacy`.
    #[derive(darudb_derive::Object, Debug, PartialEq)]
    #[darudb(collection = "members")]
    pub(crate) struct Member {
        pub(crate) id: Option<i64>,
        pub(crate) name: String,
        pub(crate) legacy: String,
    }
}

#[derive(darudb_derive::Object, Debug, PartialEq)]
#[darudb(collection = "members")]
struct Member {
    id: Option<i64>,
    name: String,
    #[darudb(default = 18)]
    age: i64,
    nickname: Option<String>,
}

#[test]
fn records_written_under_an_older_schema_read_with_defaults_and_skip_removed_fields() {
    let dir = TestDir::new();

    {
        let db = open(
            &dir,
            Schema::new(1).collection(Collection::of::<v1::Member>()),
        );
        let mut txn = db.begin_write().unwrap();

        txn.collection_of::<v1::Member>()
            .unwrap()
            .insert(&v1::Member {
                id: None,
                name: "Ada".to_owned(),
                legacy: "kept in the record".to_owned(),
            })
            .unwrap();
        txn.commit().unwrap();
        db.close().unwrap();
    }

    let mut options = OpenOptions::new();

    options
        .schema(Schema::new(2).collection(Collection::of::<Member>()))
        .migration(Migration::to(2));

    let db = options.open(dir.path("app.darudb")).unwrap();
    let read = db.begin_read().unwrap();

    assert_eq!(
        read.collection_of::<Member>().unwrap().get(1).unwrap(),
        Some(Member {
            id: Some(1),
            name: "Ada".to_owned(),
            age: 18,
            nickname: None,
        })
    );
}

#[test]
fn a_migration_function_writes_typed_objects() {
    let dir = TestDir::new();

    {
        let db = open(
            &dir,
            Schema::new(1).collection(Collection::of::<v1::Member>()),
        );
        let mut txn = db.begin_write().unwrap();

        txn.collection_of::<v1::Member>()
            .unwrap()
            .insert(&v1::Member {
                id: None,
                name: "Grace".to_owned(),
                legacy: "Amazing Grace".to_owned(),
            })
            .unwrap();
        txn.commit().unwrap();
        db.close().unwrap();
    }

    let mut options = OpenOptions::new();

    options
        .schema(Schema::new(2).collection(Collection::of::<Member>()))
        .migration(Migration::to(2).run(|migrating| {
            let old = migrating.previous("members", 1)?.unwrap();
            let legacy = old.get("legacy").and_then(Value::as_str).map(str::to_owned);
            let mut members = migrating.collection_of::<Member>()?;
            let mut member = members.get(1)?.unwrap();

            member.nickname = legacy;
            members.put(&member)?;

            Ok(())
        }));

    let db = options.open(dir.path("app.darudb")).unwrap();
    let read = db.begin_read().unwrap();
    let member = read
        .collection_of::<Member>()
        .unwrap()
        .get(1)
        .unwrap()
        .unwrap();

    assert_eq!(member.nickname.as_deref(), Some("Amazing Grace"));
}
