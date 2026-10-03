//! Measures the object layer on this machine: writing objects with their
//! indexes, reading them by key and by index, queries with and without an
//! index, updates and deletes, on a plain file and on an encrypted one.
//!
//! ```text
//! cargo run -p darudb --release --example object_bench [directory]
//! ```
//!
//! The files go into `directory`, the system's temporary directory by
//! default, and are removed afterwards. Like `kernel_bench`, the numbers are
//! for comparing builds on one machine.
//!
//! The workloads are spelled out here so that the same ones can be run
//! against other databases, outside this repository, at the same durability:
//! a sync commit waits for the disk, a deferred one does not.
//!
//! - A collection `people` of 100,000 objects keyed by an auto-increment:
//!   `name` (string, `person <n>`), `email` (string, `<n>@example.com`,
//!   unique index), `age` (int, `n * 7919 mod 80`, index), `city` (string,
//!   `city <n mod 100>`, no index) and `score` (float, `n * 0.618 mod 1`, no
//!   index), where `n` counts from 0.
//! - Inserting: one object per sync commit, one per deferred commit, and all
//!   of them in one transaction.
//! - Reading, in random order: by primary key, and by email through the
//!   unique index.
//! - Querying: `age == a` (1,250 objects each), `age BETWEEN a AND a + 4`
//!   sorted by age descending with a limit of 20, counting `age >= 40`,
//!   `city == c` (no index, 1,000 objects each), the 10 highest `score` (no
//!   index), and `age == $0 LIMIT 10` parsed from text each time.
//! - Changing: 10,000 objects' ages in one transaction, and deleting 10,000
//!   objects in one transaction.
//!
//! The reads run again through a struct that `#[derive(Object)]` makes the
//! collection's type, read straight from the records, which the lines marked
//! "typed" measure.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use darudb::{Collection, Filter, Object, OpenOptions, Query, Schema, Type, Value};

/// The objects of `people`, as a struct.
#[derive(darudb_derive::Object, Debug)]
#[darudb(collection = "people")]
struct Person {
    id: Option<i64>,
    name: String,
    email: String,
    age: i64,
    city: String,
    score: f64,
}

/// Objects in the read and query workloads.
const OBJECTS: i64 = 100_000;

type Outcome = Result<(), Box<dyn Error>>;

fn main() -> Outcome {
    let directory = env::args()
        .nth(1)
        .map_or_else(env::temp_dir, PathBuf::from)
        .join(format!("darudb-object-bench-{}", process::id()));

    fs::create_dir_all(&directory)?;

    let mut plain = OpenOptions::new();
    let mut encrypted = OpenOptions::new();

    plain.schema(schema());
    encrypted.schema(schema()).key([0x42; 32]);

    let outcome = run(&directory.join("plain"), "plain file", &plain).and_then(|()| {
        run(
            &directory.join("encrypted"),
            "encrypted file, this machine's cipher",
            &encrypted,
        )
    });

    fs::remove_dir_all(&directory)?;

    outcome
}

fn schema() -> Schema {
    Schema::new(1).collection(
        Collection::new("people")
            .field("name", Type::String)
            .field("email", Type::String)
            .field("age", Type::Int)
            .field("city", Type::String)
            .field("score", Type::Float)
            .unique("email")
            .index("age"),
    )
}

fn person(n: i64) -> Object {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the counts stay far below 2^52, where f64 is exact"
    )]
    let score = (n as f64 * 0.618).fract();

    Object::new()
        .with("name", format!("person {n}"))
        .with("email", format!("{n}@example.com"))
        .with("age", n * 7919 % 80)
        .with("city", format!("city {}", n % 100))
        .with("score", score)
}

fn run(directory: &Path, label: &str, options: &OpenOptions) -> Outcome {
    fs::create_dir_all(directory)?;
    println!("\n{label:<48} {:>12} {:>14}", "per second", "each");

    let db = options.open(directory.join("commits.darudb"))?;

    measure("insert, one object per sync commit", 500, |round| {
        let mut txn = db.begin_write()?;

        txn.collection("people")?.insert(person(signed(round)))?;
        txn.commit()
    })?;
    measure("insert, one object per deferred commit", 10_000, |round| {
        let mut txn = db.begin_write()?;

        txn.collection("people")?
            .insert(person(1_000 + signed(round)))?;
        txn.commit_deferred()
    })?;
    db.close()?;

    let db = options.open(directory.join("objects.darudb"))?;
    let mut bulk = Some(db.begin_write()?);

    measure(
        "insert in one transaction, two indexes",
        unsigned(OBJECTS),
        |round| match &mut bulk {
            Some(txn) => txn
                .collection("people")?
                .insert(person(signed(round)))
                .map(drop),
            None => Ok(()),
        },
    )?;
    measure("commit of that transaction", 1, |_| {
        bulk.take().map_or(Ok(()), darudb::WriteTransaction::commit)
    })?;

    let read = db.begin_read()?;
    let people = read.collection("people")?;

    measure(
        "get by primary key, random order",
        unsigned(OBJECTS),
        |round| {
            people
                .get(1 + signed(scatter(round) % unsigned(OBJECTS)))
                .map(drop)
        },
    )?;
    measure("get by unique email, random order", 20_000, |round| {
        let n = scatter(round) % unsigned(OBJECTS);

        people
            .query(&Query::new().filter(Filter::eq("email", format!("{n}@example.com"))))
            .map(drop)
    })?;
    measure("query age == a, 1250 objects", 200, |round| {
        people
            .query(&Query::new().filter(Filter::eq("age", signed(round % 80))))
            .map(drop)
    })?;
    measure(
        "query age range, sorted descending, limit 20",
        5_000,
        |round| {
            let age = signed(round % 76);

            people
                .query(
                    &Query::new()
                        .filter(Filter::between("age", age, age + 4))
                        .sort_by_desc("age")
                        .limit(20),
                )
                .map(drop)
        },
    )?;
    measure("count age >= 40 through the index", 200, |_| {
        people
            .count(&Query::new().filter(Filter::ge("age", 40)))
            .map(drop)
    })?;
    measure("query city == c, no index, 1000 objects", 10, |round| {
        people
            .query(&Query::new().filter(Filter::eq("city", format!("city {}", round % 100))))
            .map(drop)
    })?;
    measure("top 10 by score, no index", 10, |_| {
        people
            .query(&Query::new().sort_by_desc("score").limit(10))
            .map(drop)
    })?;
    measure("parse and run age == $0 LIMIT 10", 20_000, |round| {
        let query = Query::parse("age == $0 LIMIT 10", &[Value::Int(signed(round % 80))])?;

        people.query(&query).map(drop)
    })?;
    drop(people);

    let people = read.collection_of::<Person>()?;

    measure(
        "get by primary key, random order, typed",
        unsigned(OBJECTS),
        |round| {
            people
                .get(1 + signed(scatter(round) % unsigned(OBJECTS)))
                .map(drop)
        },
    )?;
    measure(
        "get by unique email, random order, typed",
        20_000,
        |round| {
            let n = scatter(round) % unsigned(OBJECTS);

            people
                .query(&Query::new().filter(Filter::eq("email", format!("{n}@example.com"))))
                .map(drop)
        },
    )?;
    measure("query age == a, 1250 objects, typed", 200, |round| {
        people
            .query(&Query::new().filter(Filter::eq("age", signed(round % 80))))
            .map(drop)
    })?;
    measure(
        "query age range, sorted descending, limit 20, typed",
        5_000,
        |round| {
            let age = signed(round % 76);

            people
                .query(
                    &Query::new()
                        .filter(Filter::between("age", age, age + 4))
                        .sort_by_desc("age")
                        .limit(20),
                )
                .map(drop)
        },
    )?;
    drop(people);
    drop(read);

    let mut update = Some(db.begin_write()?);

    measure("update age, in one transaction", 10_000, |round| {
        let Some(txn) = &mut update else {
            return Ok(());
        };
        let mut people = txn.collection("people")?;
        let id = 1 + signed(scatter(round) % unsigned(OBJECTS));

        if let Some(mut person) = people.get(id)? {
            person.set("age", (id + 1) % 80);
            people.put(person)?;
        }

        Ok(())
    })?;
    measure("commit of that transaction", 1, |_| {
        update
            .take()
            .map_or(Ok(()), darudb::WriteTransaction::commit)
    })?;

    let mut delete = Some(db.begin_write()?);

    measure(
        "delete, in one transaction",
        10_000,
        |round| match &mut delete {
            Some(txn) => txn
                .collection("people")?
                .delete(1 + signed(round * 7))
                .map(drop),
            None => Ok(()),
        },
    )?;
    measure("commit of that transaction", 1, |_| {
        delete
            .take()
            .map_or(Ok(()), darudb::WriteTransaction::commit)
    })?;

    Ok(())
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Runs `step` `count` times and prints how fast it went.
fn measure(
    name: &str,
    count: u64,
    mut step: impl FnMut(u64) -> darudb::Result<()>,
) -> Result<(), darudb::Error> {
    let started = Instant::now();

    for round in 0..count {
        step(round)?;
    }

    let elapsed = started.elapsed();
    let seconds = elapsed.as_secs_f64();
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts stay far below 2^52, where f64 is exact"
    )]
    let rate = count as f64 / seconds;
    let each = elapsed.checked_div(u32::try_from(count).unwrap_or(u32::MAX));

    println!(
        "{name:<48} {rate:>12.0} {:>14}",
        format_duration(each.unwrap_or(Duration::ZERO))
    );

    Ok(())
}

fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos >= 1_000_000 {
        format!("{:.2} ms", duration.as_secs_f64() * 1e3)
    } else if nanos >= 1_000 {
        format!("{:.2} us", duration.as_secs_f64() * 1e6)
    } else {
        format!("{nanos} ns")
    }
}

/// Spreads consecutive numbers over the key space, so that reads land all
/// over the tree instead of in order.
fn scatter(key: u64) -> u64 {
    key.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17)
}
