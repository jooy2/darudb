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
fn a_derived_type_reads_and_writes_a_file_opened_with_its_own_schema() {
    let dir = TestDir::new();

    {
        let db = open(&dir, schema());
        let mut txn = db.begin_write().unwrap();

        txn.collection_of::<Person>()
            .unwrap()
            .insert(&ada())
            .unwrap();
        txn.commit().unwrap();
    }

    // Opened without a schema, the file's own is the one the derive was
    // declared with, so the type matches its collection as it would have.
    let db = Database::open(dir.path("app.darudb")).unwrap();

    assert_eq!(db.stored_schema(), Some(schema()));

    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();

    people.insert(&grace()).unwrap();

    let names = people
        .query(&Query::new().filter(Filter::ge("age", 0)))
        .unwrap()
        .into_iter()
        .map(|person| person.name)
        .collect::<Vec<_>>();

    assert_eq!(names, ["Ada", "Grace"]);
    txn.commit().unwrap();
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
fn changes_by_query_name_their_fields_through_the_type() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();
    let ada_id = people.insert(&ada()).unwrap();
    let grace_id = people.insert(&grace()).unwrap();
    let young = Query::new().filter(Filter::lt("age", 50));

    assert_eq!(
        people
            .change_where(&young)
            .set(Person::ACTIVE, false)
            .set(Person::AGE, 49)
            .apply()
            .unwrap(),
        1
    );
    assert_eq!(people.get(ada_id).unwrap().unwrap().age, 49);
    assert!(!people.get(ada_id).unwrap().unwrap().active);
    assert_eq!(people.get(grace_id).unwrap().unwrap().age, 85);

    // The second change of the email to one value is refused, and the first
    // is undone with it.
    assert_eq!(
        code(
            people
                .change_where(&Query::new())
                .set(Person::EMAIL, Some("same@example.com".to_owned()))
                .apply()
        ),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        people.get(ada_id).unwrap().unwrap().email.as_deref(),
        Some("ada@example.com")
    );
    assert_eq!(
        people
            .update_where(
                &Query::new().filter(Filter::gt("age", 80)),
                darudb::Object::new().with("score", 2.5)
            )
            .unwrap(),
        1
    );
    assert_eq!(people.get(grace_id).unwrap().unwrap().score, 2.5);
    assert_eq!(people.delete_where(&young).unwrap(), 1);
    assert_eq!(people.count(&Query::new()).unwrap(), 1);
    assert_eq!(people.get(ada_id).unwrap(), None);
    assert_eq!(
        code(
            people
                .change_where(&Query::new())
                .set(Person::ID, Some(9))
                .apply()
        ),
        "INVALID_ARGUMENT"
    );
}

#[test]
fn typed_conditions_are_the_filters_their_names_make() {
    // Each condition names the field the collection stores, `photo` and
    // `zip` for the renamed fields, and compares it with a value of its type.
    let pairs = [
        (Person::AGE.eq(36), Filter::eq("age", 36)),
        (Person::AGE.ne(36), Filter::ne("age", 36)),
        (Person::AGE.lt(40), Filter::lt("age", 40)),
        (Person::AGE.le(40), Filter::le("age", 40)),
        (Person::AGE.gt(40), Filter::gt("age", 40)),
        (Person::AGE.ge(40), Filter::ge("age", 40)),
        (Person::AGE.between(1, 9), Filter::between("age", 1, 9)),
        (Person::AGE.is_in([1, 2]), Filter::is_in("age", [1, 2])),
        (Person::SCORE.ge(1), Filter::ge("score", 1.0)),
        (Person::ACTIVE.eq(true), Filter::eq("active", true)),
        (Person::NAME.eq("Ada"), Filter::eq("name", "Ada")),
        (Person::NAME.contains("d"), Filter::contains("name", "d")),
        (
            Person::NAME.starts_with("A"),
            Filter::starts_with("name", "A"),
        ),
        (Person::NAME.ends_with("a"), Filter::ends_with("name", "a")),
        (Person::EMAIL.eq("a@b"), Filter::eq("email", "a@b")),
        (
            Person::EMAIL.starts_with("a"),
            Filter::starts_with("email", "a"),
        ),
        (Person::EMAIL.is_null(), Filter::is_null("email")),
        (Person::EMAIL.is_not_null(), Filter::is_not_null("email")),
        (
            Person::TAGS.contains("math"),
            Filter::contains("tags", "math"),
        ),
        (
            Person::PICTURE.eq([0u8, 1]),
            Filter::eq("photo", vec![0u8, 1]),
        ),
        (Person::FRIEND.eq(1), Filter::eq("friend", 1)),
        (
            Person::HOME.then(Address::POSTAL_CODE).eq("01"),
            Filter::eq("home.zip", "01"),
        ),
        (
            Person::HOME.then(Address::POINT).then(Point::X).gt(0),
            Filter::gt("home.point.x", 0.0),
        ),
        (
            Person::FRIEND.then(Person::NAME).eq("Ada"),
            Filter::eq("friend.name", "Ada"),
        ),
        (
            Post::AUTHOR
                .then(Person::HOME)
                .then(Address::CITY)
                .is_null(),
            Filter::is_null("author.home.city"),
        ),
        (Post::READERS.contains(2), Filter::contains("readers", 2)),
        (Post::SCORES.contains(7), Filter::contains("scores", 7)),
        (Post::AUTHOR.any(), Filter::linked("posts", "author")),
        (
            Post::READERS.any_where(Post::SLUG.starts_with("a")),
            Filter::linked_where("posts", "readers", Filter::starts_with("slug", "a")),
        ),
        (
            Person::NAME.eq("ada").ignore_case(),
            Filter::eq("name", "ada").ignore_case(),
        ),
    ];

    for (typed, named) in pairs {
        assert_eq!(typed, named);
    }

    assert_eq!(
        Query::new()
            .sort_by(Person::AGE)
            .sort_by_desc(Person::HOME.then(Address::CITY)),
        Query::new().sort_by("age").sort_by_desc("home.city")
    );
    assert_eq!(
        Person::HOME.then(Address::POSTAL_CODE).names(),
        ["home", "zip"]
    );
    assert_eq!(Person::PICTURE.name(), "photo");
    assert_eq!(Address::POSTAL_CODE.slot(), 1);
}

#[test]
fn typed_queries_and_aggregates_find_what_named_ones_do() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();
    let ada_id = people.insert(&ada()).unwrap();

    for n in 0..20_i32 {
        people
            .insert(&Person {
                name: format!("person {n}"),
                age: i64::from(n) * 3,
                score: f64::from(n) / 4.0,
                home: (n % 2 == 0).then(|| Address {
                    city: if n % 4 == 0 { "Seoul" } else { "Busan" }.to_owned(),
                    postal_code: None,
                    point: Point { x: 0.0, y: 0.0 },
                }),
                friend: Some(Link::new(ada_id)),
                ..grace()
            })
            .unwrap();
    }

    drop(people);

    let mut posts = txn.collection_of::<Post>().unwrap();

    for (slug, author, readers) in [
        ("a", ada_id, vec![2, 3]),
        ("b", 4, vec![]),
        ("c", 4, vec![2]),
    ] {
        posts
            .insert(&Post {
                slug: slug.to_owned(),
                author: Link::new(author),
                readers: readers.into_iter().map(Link::new).collect(),
                scores: Vec::new(),
            })
            .unwrap();
    }

    drop(posts);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let typed = read.collection_of::<Person>().unwrap();
    let untyped = read.collection("people").unwrap();
    let ids = |found: Vec<Person>| {
        found
            .into_iter()
            .map(|person| person.id)
            .collect::<Vec<_>>()
    };
    let pairs = [
        (
            Query::new()
                .filter(Person::HOME.then(Address::CITY).eq("Seoul"))
                .sort_by_desc(Person::AGE),
            Query::new()
                .filter(Filter::eq("home.city", "Seoul"))
                .sort_by_desc("age"),
        ),
        (
            Query::new().filter(Person::FRIEND.then(Person::NAME).eq("Ada")),
            Query::new().filter(Filter::eq("friend.name", "Ada")),
        ),
        (
            Query::new().filter(Post::AUTHOR.any().and(!Post::READERS.any())),
            Query::parse("ANY posts.author AND NOT ANY posts.readers", &[]).unwrap(),
        ),
    ];

    for (built, named) in &pairs {
        let found = ids(typed.query(built).unwrap());

        assert!(!found.is_empty(), "{built:?}");
        assert_eq!(found, ids(typed.query(named).unwrap()));
    }

    let adults = Query::new().filter(Person::AGE.ge(18));

    assert_eq!(
        typed.sum(&adults, Person::AGE).unwrap(),
        untyped.sum(&adults, "age").unwrap().as_int().unwrap()
    );
    assert_eq!(
        typed.sum(&adults, Person::SCORE).unwrap(),
        untyped.sum(&adults, "score").unwrap().as_float().unwrap()
    );
    assert_eq!(
        typed.avg(&adults, Person::AGE).unwrap(),
        untyped.avg(&adults, "age").unwrap().as_float()
    );
    assert_eq!(typed.min(&adults, Person::AGE).unwrap(), Some(18));
    assert_eq!(typed.max(&Query::new(), Person::AGE).unwrap(), Some(57));
    assert_eq!(
        typed
            .min(&Query::new(), Person::HOME.then(Address::CITY))
            .unwrap()
            .as_deref(),
        Some("Busan")
    );
    assert_eq!(
        typed.max(&Query::new(), Person::EMAIL).unwrap().as_deref(),
        Some("ada@example.com")
    );

    // An aggregate of nothing: a sum is zero, the others are none.
    let nobody = Query::new().filter(Person::AGE.gt(1000));

    assert_eq!(typed.sum(&nobody, Person::AGE).unwrap(), 0);
    assert_eq!(typed.min(&nobody, Person::AGE).unwrap(), None);
    assert_eq!(typed.avg(&nobody, Person::SCORE).unwrap(), None);

    // A constant made by hand with the name of a field of another type
    // fails when it runs, rather than giving a value of the wrong type.
    let wrong = darudb::FieldOf::<Person, i64>::new(1, "name");

    assert_eq!(code(typed.max(&Query::new(), wrong)), "INVALID_ARGUMENT");
    assert_eq!(code(typed.sum(&Query::new(), wrong)), "INVALID_QUERY");
}

#[test]
fn set_and_change_write_fields_named_through_the_type() {
    let dir = TestDir::new();
    let db = open(&dir, schema());
    let mut txn = db.begin_write().unwrap();
    let mut people = txn.collection_of::<Person>().unwrap();
    let ada_id = people.insert(&ada()).unwrap();
    let grace_id = people.insert(&grace()).unwrap();

    assert!(people.set(ada_id, Person::AGE, 40).unwrap());
    assert!(
        people
            .change(ada_id)
            .set(Person::NAME, "Ada Lovelace".to_owned())
            .set(Person::EMAIL, None)
            .set(Person::TAGS, vec!["poetry".to_owned()])
            .set(Person::FRIEND, Some(Link::new(grace_id)))
            .set(Person::AGE, 41)
            .apply()
            .unwrap()
    );
    // A collection with embedded objects and lists takes the changes as an
    // update with an object does.
    assert!(
        people
            .set(
                grace_id,
                Person::HOME,
                Some(Address {
                    city: "Arlington".to_owned(),
                    postal_code: Some("22201".to_owned()),
                    point: Point { x: 1.0, y: 2.0 },
                }),
            )
            .unwrap()
    );
    assert!(!people.set(99, Person::AGE, 1).unwrap());

    let expected = Person {
        id: Some(ada_id),
        name: "Ada Lovelace".to_owned(),
        email: None,
        age: 41,
        tags: vec!["poetry".to_owned()],
        friend: Some(Link::new(grace_id)),
        ..ada()
    };

    assert_eq!(people.get(ada_id).unwrap(), Some(expected));
    assert_eq!(
        people.get(grace_id).unwrap().unwrap().home.unwrap().city,
        "Arlington"
    );
    // The index on the age followed the change.
    assert_eq!(
        people
            .query(&Query::new().filter(Filter::eq("age", 41)))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        code(people.set(grace_id, Person::ID, Some(ada_id))),
        "INVALID_ARGUMENT"
    );
}

#[test]
fn set_changes_a_record_of_scalars_and_refuses_a_taken_unique_value() {
    let dir = TestDir::new();
    let db = open(&dir, Schema::new(1).collection(Collection::of::<Account>()));
    let mut txn = db.begin_write().unwrap();
    let mut accounts = txn.collection_of::<Account>().unwrap();
    let account = |email: &str, level| Account {
        id: None,
        email: email.to_owned(),
        level,
        note: Some("first".to_owned()),
    };
    let a = accounts.insert(&account("a@example.com", 1)).unwrap();
    let b = accounts.insert(&account("b@example.com", 2)).unwrap();

    assert!(
        accounts
            .change(a)
            .set(Account::LEVEL, 5)
            .set(Account::NOTE, None)
            .apply()
            .unwrap()
    );
    assert_eq!(
        code(accounts.set(b, Account::EMAIL, "a@example.com".to_owned())),
        "DUPLICATE_KEY"
    );
    // The refused change left the object as it was.
    assert_eq!(
        accounts.get(b).unwrap(),
        Some(Account {
            id: Some(b),
            ..account("b@example.com", 2)
        })
    );
    assert_eq!(
        accounts.get(a).unwrap(),
        Some(Account {
            id: Some(a),
            email: "a@example.com".to_owned(),
            level: 5,
            note: None,
        })
    );
    assert_eq!(
        accounts
            .query(&Query::new().filter(Filter::eq("level", 5)))
            .unwrap()
            .len(),
        1
    );
    assert!(
        accounts
            .query(&Query::new().filter(Filter::eq("level", 1)))
            .unwrap()
            .is_empty()
    );
    drop(accounts);
    txn.commit().unwrap();
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

/// A type whose fields all hold scalars, whose objects a typed write stores
/// as it encodes them.
#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "accounts")]
struct Account {
    id: Option<i64>,
    #[darudb(unique)]
    email: String,
    #[darudb(index)]
    level: i64,
    note: Option<String>,
}

#[test]
fn a_type_of_scalars_numbers_indexes_and_refuses_as_objects_are() {
    let dir = TestDir::new();
    let db = open(&dir, Schema::new(1).collection(Collection::of::<Account>()));
    let account = |email: &str, level| Account {
        id: None,
        email: email.to_owned(),
        level,
        note: None,
    };
    let mut txn = db.begin_write().unwrap();
    let mut accounts = txn.collection_of::<Account>().unwrap();

    assert_eq!(accounts.insert(&account("a@example.com", 1)).unwrap(), 1);
    // A key of its own moves the numbering past it.
    assert_eq!(
        accounts
            .insert(&Account {
                id: Some(10),
                ..account("b@example.com", 2)
            })
            .unwrap(),
        10
    );
    assert_eq!(accounts.insert(&account("c@example.com", 2)).unwrap(), 11);
    assert_eq!(
        code(accounts.insert(&account("a@example.com", 3))),
        "DUPLICATE_KEY"
    );
    assert_eq!(
        accounts
            .put(&Account {
                id: Some(1),
                note: Some("moved".to_owned()),
                ..account("d@example.com", 2)
            })
            .unwrap(),
        1
    );
    assert_eq!(accounts.insert(&account("a@example.com", 3)).unwrap(), 12);
    drop(accounts);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let accounts = read.collection_of::<Account>().unwrap();
    let level = |level: i64| {
        let mut ids: Vec<_> = accounts
            .query(&Query::new().filter(Filter::eq("level", level)))
            .unwrap()
            .into_iter()
            .filter_map(|account| account.id)
            .collect();

        ids.sort_unstable();
        ids
    };

    assert_eq!(level(1), Vec::<i64>::new());
    assert_eq!(level(2), vec![1, 10, 11]);
    assert_eq!(level(3), vec![12]);
    assert_eq!(
        accounts
            .query(&Query::new().filter(Filter::eq("email", "d@example.com")))
            .unwrap()
            .first()
            .and_then(|account| account.note.clone()),
        Some("moved".to_owned())
    );
    assert_eq!(
        read.collection("accounts").unwrap().get(12).unwrap(),
        Some(
            Object::new()
                .with("id", 12)
                .with("email", "a@example.com")
                .with("level", 3)
                .with("note", Value::Null)
        )
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

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "events")]
struct Event {
    id: Option<i64>,
    #[darudb(index)]
    at: std::time::SystemTime,
    ends: Option<std::time::SystemTime>,
    seen: Vec<std::time::SystemTime>,
}

#[test]
fn a_system_time_is_a_date_rounded_down_to_the_microsecond() {
    use std::time::{Duration, UNIX_EPOCH};

    let dir = TestDir::new();
    let db = open(&dir, Schema::new(1).collection(Collection::of::<Event>()));
    let launch = UNIX_EPOCH + Duration::new(1_793_361_600, 123_456_789);
    let before = UNIX_EPOCH - Duration::new(0, 1_500);
    let micros = |time: std::time::SystemTime| Value::try_from(time).unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut events = txn.collection_of::<Event>().unwrap();

    for (at, ends) in [(launch, None), (before, Some(launch)), (UNIX_EPOCH, None)] {
        events
            .insert(&Event {
                id: None,
                at,
                ends,
                seen: vec![at, UNIX_EPOCH],
            })
            .unwrap();
    }

    drop(events);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let events = read.collection_of::<Event>().unwrap();
    let first = events.get(1).unwrap().unwrap();
    let second = events.get(2).unwrap().unwrap();

    // Down to the microsecond: 789 nanoseconds after, and 500 before.
    assert_eq!(
        first.at,
        UNIX_EPOCH + Duration::new(1_793_361_600, 123_456_000)
    );
    assert_eq!(second.at, UNIX_EPOCH - Duration::from_micros(2));
    assert_eq!(second.ends, Some(first.at));
    assert_eq!(second.seen, [second.at, UNIX_EPOCH]);
    assert_eq!(micros(second.at), Value::Date(-2));

    let found = |query: &Query| {
        events
            .query(query)
            .unwrap()
            .into_iter()
            .map(|event| event.id.unwrap())
            .collect::<Vec<_>>()
    };

    assert_eq!(found(&Query::new().filter(Event::AT.lt(UNIX_EPOCH))), [2]);
    assert_eq!(
        found(
            &Query::new()
                .filter(Event::AT.ge(UNIX_EPOCH))
                .sort_by_desc(Event::AT)
        ),
        [1, 3]
    );
    assert_eq!(found(&Query::new().filter(Event::ENDS.is_not_null())), [2]);
    // A condition rounds its time down as a write does.
    assert_eq!(
        found(&Query::new().filter(Event::SEEN.contains(launch))),
        [1]
    );
    assert_eq!(
        found(&Query::new().filter(Event::SEEN.contains(first.at))),
        [1]
    );
    assert_eq!(
        events.min(&Query::new(), Event::AT).unwrap(),
        Some(second.at)
    );
    assert_eq!(
        events.max(&Query::new(), Event::ENDS).unwrap(),
        Some(first.at)
    );

    // A time past what a date holds compares as the last date there is, on
    // a platform whose `SystemTime` reaches that far, which Windows does not.
    if let Some(far) = UNIX_EPOCH.checked_add(Duration::from_secs(u64::MAX / 4)) {
        assert_eq!(found(&Query::new().filter(Event::AT.lt(far))), [1, 2, 3]);
        assert_eq!(code(Value::try_from(far)), "INVALID_ARGUMENT");
    }
}

#[derive(darudb_derive::Embedded, Debug, Clone, PartialEq)]
struct Stop {
    city: String,
    #[darudb(rename = "mins")]
    minutes: Option<i64>,
}

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "trips")]
struct Trip {
    id: Option<i64>,
    name: String,
    stops: Vec<Stop>,
}

#[test]
fn a_list_of_embedded_objects_reads_into_a_vec_of_its_type() {
    let dir = TestDir::new();
    let db = open(&dir, Schema::new(1).collection(Collection::of::<Trip>()));
    let stop = |city: &str, minutes: Option<i64>| Stop {
        city: city.to_owned(),
        minutes,
    };
    let coast = Trip {
        id: None,
        name: "coast".to_owned(),
        stops: vec![stop("Busan", Some(30)), stop("Ulsan", None)],
    };
    let mut txn = db.begin_write().unwrap();
    let mut trips = txn.collection_of::<Trip>().unwrap();

    trips.insert(&coast).unwrap();
    trips
        .insert(&Trip {
            id: None,
            name: "none".to_owned(),
            stops: Vec::new(),
        })
        .unwrap();
    drop(trips);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let typed = read.collection_of::<Trip>().unwrap();
    let untyped = read.collection("trips").unwrap();

    assert_eq!(
        typed.get(1).unwrap().unwrap(),
        Trip {
            id: Some(1),
            ..coast.clone()
        }
    );
    // Untyped, each stop is an object by the names the file stores.
    assert_eq!(
        untyped.get(1).unwrap().unwrap().get("stops"),
        Some(&Value::List(vec![
            Value::Object(Object::new().with("city", "Busan").with("mins", 30)),
            Value::Object(
                Object::new()
                    .with("city", "Ulsan")
                    .with("mins", Value::Null)
            ),
        ]))
    );

    let ids = |query: &Query| {
        typed
            .query(query)
            .unwrap()
            .into_iter()
            .map(|trip| trip.id.unwrap())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        Trip::STOPS.then(Stop::MINUTES).gt(10),
        Filter::gt("stops.mins", 10)
    );
    assert_eq!(
        ids(&Query::new().filter(Trip::STOPS.then(Stop::CITY).eq("Ulsan"))),
        [1]
    );
    assert_eq!(
        ids(&Query::new().filter(Trip::STOPS.then(Stop::MINUTES).is_null())),
        [1]
    );
    // No stop of an empty list is null, as no element of it is anything.
    assert_eq!(
        ids(&Query::new().filter(Trip::STOPS.then(Stop::CITY).is_not_null())),
        [1, 2]
    );
    // A list is not sorted by, nor aggregated over.
    assert_eq!(
        code(typed.query(&Query::new().sort_by(Trip::STOPS.then(Stop::CITY)))),
        "INVALID_QUERY"
    );
    assert_eq!(
        code(typed.max(&Query::new(), Trip::STOPS.then(Stop::MINUTES))),
        "INVALID_QUERY"
    );
}

#[derive(darudb_derive::Embedded, Debug, Clone, PartialEq)]
struct Place {
    city: String,
}

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "residents")]
#[darudb(unique(last, first), index("home.city"))]
struct Resident {
    id: Option<i64>,
    last: String,
    #[darudb(rename = "given")]
    first: Option<String>,
    home: Option<Place>,
}

#[test]
fn a_derived_type_declares_indexes_on_several_fields_and_on_paths() {
    let dir = TestDir::new();
    let declared = Collection::of::<Resident>();

    assert_eq!(
        declared
            .indexes()
            .iter()
            .map(|index| (index.fields().collect::<Vec<_>>(), index.is_unique()))
            .collect::<Vec<_>>(),
        [
            (vec!["last".to_owned(), "given".to_owned()], true),
            (vec!["home.city".to_owned()], false),
        ]
    );

    let db = open(&dir, Schema::new(1).collection(declared));
    let member = |last: &str, first: &str, city: &str| Resident {
        id: None,
        last: last.to_owned(),
        first: Some(first.to_owned()),
        home: Some(Place {
            city: city.to_owned(),
        }),
    };
    let mut txn = db.begin_write().unwrap();
    let mut members = txn.collection_of::<Resident>().unwrap();

    members.insert(&member("Kim", "Ada", "Seoul")).unwrap();
    members.insert(&member("Kim", "Bo", "Busan")).unwrap();
    assert_eq!(
        code(members.insert(&member("Kim", "Ada", "Jeju"))),
        "DUPLICATE_KEY"
    );
    drop(members);
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let members = read.collection_of::<Resident>().unwrap();

    assert_eq!(
        members
            .query(&Query::new().filter(Resident::HOME.then(Place::CITY).eq("Busan")))
            .unwrap()
            .into_iter()
            .map(|member| member.first.unwrap())
            .collect::<Vec<_>>(),
        ["Bo"]
    );
    assert_eq!(
        members
            .count(
                &Query::new()
                    .filter(Resident::LAST.eq("Kim"))
                    .sort_by(Resident::FIRST)
            )
            .unwrap(),
        2
    );
}

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "clubs")]
struct Club {
    id: Option<i64>,
    name: String,
}

#[derive(darudb_derive::Object, Debug, Clone, PartialEq)]
#[darudb(collection = "joined")]
struct Joined {
    id: Option<i64>,
    #[darudb(index, cascade)]
    club: Link<Club>,
    #[darudb(cascade)]
    sponsor: Option<Link<Joined>>,
}

/// `#[darudb(cascade)]` on a link and on an optional link, with the typed
/// deletes going through them.
#[test]
fn a_derived_link_cascades() {
    let dir = TestDir::new();
    let path = dir.path("objects.darudb");
    let declared = Collection::of::<Joined>();

    assert!(
        declared
            .fields()
            .iter()
            .all(|field| field.is_cascade() == (field.name() != "id"))
    );

    let db = OpenOptions::new()
        .schema(
            Schema::new(1)
                .collection(Collection::of::<Club>())
                .collection(declared),
        )
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();
    let mut clubs = txn.collection_of::<Club>().unwrap();

    for name in ["chess", "go"] {
        clubs
            .insert(&Club {
                id: None,
                name: name.to_owned(),
            })
            .unwrap();
    }

    drop(clubs);

    let mut joined = txn.collection_of::<Joined>().unwrap();
    let member = |club, sponsor: Option<i64>| Joined {
        id: None,
        club: Link::new(club),
        sponsor: sponsor.map(Link::new),
    };

    // 2 goes with 1, its sponsor, though its club stays.
    joined.insert(&member(1, None)).unwrap();
    joined.insert(&member(2, Some(1))).unwrap();
    joined.insert(&member(2, None)).unwrap();
    drop(joined);

    assert!(txn.collection_of::<Club>().unwrap().delete(1).unwrap());
    assert_eq!(
        txn.collection_of::<Joined>()
            .unwrap()
            .query(&Query::new())
            .unwrap()
            .into_iter()
            .map(|member| member.id)
            .collect::<Vec<_>>(),
        [Some(3)]
    );

    let found = txn
        .collection_of::<Club>()
        .unwrap()
        .delete_where(&Query::new())
        .unwrap();

    assert_eq!(found, 1);
    assert_eq!(txn.collection_of::<Joined>().unwrap().len().unwrap(), 0);
    txn.commit().unwrap();
}
