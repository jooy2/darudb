//! What the fuzz targets share: a small database with every kind of field
//! and index, and the calls that run a query against it.
//!
//! A target fails by panicking, by overflowing in arithmetic, or by using
//! too much memory; an error the engine returns is the right answer to an
//! input that makes no sense.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use darudb::{Collection, Database, Embedded, Object, OpenOptions, Query, Schema, Type, Value};

/// A schema with a field of every type, an index of every kind, a link to
/// the collection itself and one to another collection.
pub fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("teams")
                .primary_key("name", Type::String)
                .optional("city", Type::String),
        )
        .collection(
            Collection::new("people")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .optional("score", Type::Float)
                .with_default("active", Type::Bool, true)
                .optional("photo", Type::Bytes)
                .optional("born", Type::Date)
                .optional("tags", Type::list(Type::String))
                .optional(
                    "home",
                    Type::object(
                        Embedded::new()
                            .field("city", Type::String)
                            .optional("zip", Type::String),
                    ),
                )
                .optional(
                    "stops",
                    Type::list(Type::object(Embedded::new().field("city", Type::String))),
                )
                .optional("team", Type::link("teams"))
                .optional("friend", Type::link("people"))
                .optional("friends", Type::list(Type::link("people")))
                .unique("email")
                .index("age")
                .index("tags")
                .index("team")
                .index("friend")
                .index_on(["age", "name"])
                .index_on(["home.city"])
                .cascade("friend"),
        )
}

/// Fills `db`, opened with [`schema`], with a few teams and `people` people,
/// in one transaction, one of them with a value too large for a leaf.
pub fn fill(db: &Database, people: i64) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;

    for (name, city) in [("north", "Seoul"), ("south", "Busan")] {
        txn.collection("teams")?
            .insert(Object::new().with("name", name).with("city", city))?;
    }

    let mut all = txn.collection("people")?;

    for n in 0..people {
        let mut person = Object::new()
            .with("name", format!("person {n}"))
            .with("age", n % 7)
            .with("score", n as f64 / 3.0)
            .with("active", n % 2 == 0)
            .with("born", Value::Date(n * 86_400_000_000))
            .with(
                "tags",
                vec![Value::from(format!("t{}", n % 3)), Value::from("all")],
            )
            .with(
                "home",
                Object::new().with("city", if n % 2 == 0 { "Seoul" } else { "Busan" }),
            )
            .with("team", if n % 2 == 0 { "north" } else { "south" });

        if n % 3 != 0 {
            person.set("email", format!("{n}@example.com"));
        }

        if n > 0 {
            person.set("friend", n);
            person.set("friends", vec![Value::from(1), Value::from(n)]);
        }

        if n == 3 {
            person.set("photo", vec![7u8; 9000]);
            person.set(
                "stops",
                vec![Value::from(Object::new().with("city", "Daegu"))],
            );
        }

        all.insert(person)?;
    }

    txn.commit()
}

/// A directory this process alone uses, kept until the process ends.
pub fn directory() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

    DIRECTORY.get_or_init(|| tempfile::tempdir().expect("a temporary directory").keep())
}

/// The database the query and record targets run against, opened once.
pub fn database() -> &'static Database {
    static DATABASE: OnceLock<Database> = OnceLock::new();

    DATABASE.get_or_init(|| {
        let db = OpenOptions::new()
            .schema(schema())
            .open(directory().join("fixed.darudb"))
            .expect("the database opens");

        fill(&db, 40).expect("the database fills");
        db
    })
}

/// Runs `query` against the people every way a caller can: the objects, the
/// count, and each aggregate of two fields. Errors are answers.
pub fn run(query: &Query) {
    let Ok(read) = database().begin_read() else {
        return;
    };
    let Ok(people) = read.collection("people") else {
        return;
    };
    let _ = people.query(query);
    let _ = people.count(query);

    for field in ["age", "name"] {
        let _ = people.sum(query, field);
        let _ = people.min(query, field);
        let _ = people.max(query, field);
        let _ = people.avg(query, field);
    }
}
