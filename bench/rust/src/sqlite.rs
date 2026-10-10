//! SQLite through rusqlite, with SQLite bundled. A write-ahead log, with
//! `synchronous = FULL` for the sync commits and `NORMAL` for the deferred
//! ones; `fullfsync` is on, so that on Apple systems SQLite flushes the way
//! DaruDB does, and it changes nothing elsewhere. The page cache is 32 MiB,
//! DaruDB's default. Reads run in one transaction through prepared
//! statements, as DaruDB's run in one read transaction.

use std::path::Path;

use rusqlite::{Connection, Statement, params};

use crate::work::{Digest, OBJECTS, Person, Rows, person, random_id, random_number};

const COLUMNS: &str = "id, name, email, age, city, score";

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;

    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA fullfsync = ON;
         PRAGMA cache_size = -32768;
         CREATE TABLE IF NOT EXISTS people (
           id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
           age INTEGER NOT NULL, city TEXT NOT NULL, score REAL NOT NULL);
         CREATE INDEX IF NOT EXISTS people_age ON people (age DESC);",
    )?;
    Ok(connection)
}

fn insert(connection: &Connection, p: &Person) -> rusqlite::Result<()> {
    connection
        .prepare_cached(
            "INSERT INTO people (name, email, age, city, score) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![p.name, p.email, p.age, p.city, p.score])
        .map(drop)
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get(0)?,
        name: row.get(1)?,
        email: row.get(2)?,
        age: row.get(3)?,
        city: row.get(4)?,
        score: row.get(5)?,
    })
}

fn each(
    statement: &mut Statement<'_>,
    parameters: impl rusqlite::Params,
    d: &mut Digest,
) -> rusqlite::Result<()> {
    let mut rows = statement.query(parameters)?;

    while let Some(found) = rows.next()? {
        d.person(&row(found)?);
    }
    Ok(())
}

pub fn run(directory: &Path, rows: &mut Rows) -> rusqlite::Result<()> {
    let connection = open(&directory.join("commits.sqlite"))?;

    rows.each("insert-sync", 500, |round, _| {
        let txn = connection.unchecked_transaction()?;

        insert(&txn, &person(round as i64))?;
        txn.commit()
    });
    connection.execute_batch("PRAGMA synchronous = NORMAL")?;
    rows.each("insert-deferred", 10_000, |round, _| {
        let txn = connection.unchecked_transaction()?;

        insert(&txn, &person(1_000 + round as i64))?;
        txn.commit()
    });
    connection.close().map_err(|(_, error)| error)?;

    let connection = open(&directory.join("objects.sqlite"))?;

    rows.all("insert-bulk", OBJECTS as u64, |_| {
        connection.execute_batch("BEGIN")?;

        for n in 0..OBJECTS {
            insert(&connection, &person(n))?;
        }

        connection.execute_batch("COMMIT")
    });

    connection.execute_batch("BEGIN")?;
    {
        let mut by_id =
            connection.prepare_cached(&format!("SELECT {COLUMNS} FROM people WHERE id = ?1"))?;
        let mut by_email =
            connection.prepare_cached(&format!("SELECT {COLUMNS} FROM people WHERE email = ?1"))?;
        let mut by_age =
            connection.prepare_cached(&format!("SELECT {COLUMNS} FROM people WHERE age = ?1"))?;
        let mut range = connection.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM people WHERE age BETWEEN ?1 AND ?2 ORDER BY age DESC, id ASC LIMIT 20"
        ))?;
        let mut count = connection.prepare_cached("SELECT count(*) FROM people WHERE age >= ?1")?;
        let mut city =
            connection.prepare_cached(&format!("SELECT {COLUMNS} FROM people WHERE city = ?1"))?;
        let mut top = connection.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM people ORDER BY score DESC, id ASC LIMIT 10"
        ))?;

        rows.each("get-key", OBJECTS as u64, |round, d| {
            each(&mut by_id, [random_id(round)], d)
        });
        rows.each("get-email", 20_000, |round, d| {
            each(
                &mut by_email,
                [format!("{}@example.com", random_number(round))],
                d,
            )
        });
        rows.each("age-equal", 200, |round, d| {
            each(&mut by_age, [(round % 80) as i64], d)
        });
        rows.each("age-range", 5_000, |round, d| {
            let age = (round % 76) as i64;

            each(&mut range, [age, age + 4], d)
        });
        rows.each("count", 200, |_, d| {
            let n: i64 = count.query_row([40], |r| r.get(0))?;

            d.number(n as u64);
            Ok::<_, rusqlite::Error>(())
        });
        rows.each("city-scan", 10, |round, d| {
            each(&mut city, [format!("city {}", round % 100)], d)
        });
        rows.each("top-score", 10, |_, d| each(&mut top, [], d));
    }
    connection.execute_batch("COMMIT")?;

    rows.all("update", 10_000, |_| {
        connection.execute_batch("BEGIN")?;

        let mut get =
            connection.prepare_cached(&format!("SELECT {COLUMNS} FROM people WHERE id = ?1"))?;
        let mut set = connection.prepare_cached("UPDATE people SET age = ?1 WHERE id = ?2")?;

        for round in 0..10_000 {
            let id = random_id(round);

            match get.query_row([id], row) {
                Ok(mut p) => {
                    p.age = (id + 1) % 80;
                    set.execute([p.age, p.id])?;
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(error) => return Err(error),
            }
        }

        drop((get, set));
        connection.execute_batch("COMMIT")
    });
    rows.all("delete", 10_000, |d| {
        connection.execute_batch("BEGIN")?;

        let mut delete = connection.prepare_cached("DELETE FROM people WHERE id = ?1")?;

        for round in 0..10_000 {
            if delete.execute([1 + round * 7])? == 1 {
                d.number(1);
            }
        }

        drop(delete);
        connection.execute_batch("COMMIT")
    });

    let mut left = Digest::default();

    each(
        &mut connection.prepare(&format!("SELECT {COLUMNS} FROM people ORDER BY id"))?,
        [],
        &mut left,
    )?;
    rows.check("left", left);
    Ok(())
}
