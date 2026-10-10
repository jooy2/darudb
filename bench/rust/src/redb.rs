//! redb. A sync commit is `Durability::Immediate`; a deferred one is
//! `Durability::None`, which promises less than a deferred commit: nothing
//! it writes is durable until a later immediate commit, which the run makes
//! once at the end. The unique and age indexes are tables of their own,
//! written by hand as an application would.

use std::path::Path;

use redb::{Database, Durability, ReadableDatabase, ReadableTable, Table, TableDefinition};

use crate::work::{Digest, OBJECTS, Person, Rows, Top, person, random_id, random_number, record};

const PEOPLE: TableDefinition<u64, &[u8]> = TableDefinition::new("people");
const EMAIL: TableDefinition<&str, u64> = TableDefinition::new("email");
const AGE: TableDefinition<&[u8], ()> = TableDefinition::new("age");

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn open(path: &Path) -> Result<Database, Box<dyn std::error::Error>> {
    let db = Database::create(path)?;
    let txn = db.begin_write()?;

    txn.open_table(PEOPLE)?;
    txn.open_table(EMAIL)?;
    txn.open_table(AGE)?;
    txn.commit()?;
    Ok(db)
}

/// The tables of one write transaction, opened once.
struct Tables<'t> {
    people: Table<'t, u64, &'static [u8]>,
    email: Table<'t, &'static str, u64>,
    age: Table<'t, &'static [u8], ()>,
}

impl<'t> Tables<'t> {
    fn open(txn: &'t redb::WriteTransaction) -> Result<Self, redb::TableError> {
        Ok(Self {
            people: txn.open_table(PEOPLE)?,
            email: txn.open_table(EMAIL)?,
            age: txn.open_table(AGE)?,
        })
    }

    fn insert(&mut self, p: &Person, buffer: &mut Vec<u8>) -> Outcome {
        let id = self.people.last()?.map_or(1, |(key, _)| key.value() + 1);

        record::encode(p, buffer);
        self.people.insert(id, buffer.as_slice())?;
        self.email.insert(p.email.as_str(), id)?;
        self.age
            .insert(&record::age_key(p.age, id as i64)[..], ())?;
        Ok(())
    }
}

pub fn run(directory: &Path, rows: &mut Rows) -> Outcome {
    let mut buffer = Vec::new();
    let db = open(&directory.join("commits.redb"))?;

    rows.each("insert-sync", 500, |round, _| {
        let mut txn = db.begin_write()?;

        txn.set_durability(Durability::Immediate)?;
        Tables::open(&txn)?.insert(&person(round as i64), &mut buffer)?;
        txn.commit()?;
        Outcome::Ok(())
    });
    rows.each("insert-deferred", 10_000, |round, _| {
        let mut txn = db.begin_write()?;

        txn.set_durability(Durability::None)?;
        Tables::open(&txn)?.insert(&person(1_000 + round as i64), &mut buffer)?;
        txn.commit()?;
        Outcome::Ok(())
    });
    db.begin_write()?.commit()?;
    drop(db);

    let db = open(&directory.join("objects.redb"))?;

    rows.all("insert-bulk", OBJECTS as u64, |_| {
        let txn = db.begin_write()?;
        let mut tables = Tables::open(&txn)?;

        for n in 0..OBJECTS {
            tables.insert(&person(n), &mut buffer)?;
        }

        drop(tables);
        txn.commit()?;
        Outcome::Ok(())
    });

    let txn = db.begin_read()?;
    let people = txn.open_table(PEOPLE)?;
    let email = txn.open_table(EMAIL)?;
    let ages = txn.open_table(AGE)?;
    let get = |id: i64| -> Result<Option<Person>, redb::StorageError> {
        Ok(people
            .get(id as u64)?
            .map(|bytes| record::decode(id, bytes.value())))
    };
    let of_age = |age: i64, limit: usize, d: &mut Digest| -> Result<usize, redb::StorageError> {
        let low = record::age_key(age, 0);
        let high = record::age_key(age + 1, 0);
        let mut given = 0;

        for entry in ages.range(&low[..]..&high[..])? {
            if given == limit {
                break;
            }

            let (key, _) = entry?;

            d.person(&get(record::id_of(key.value()))?.expect("indexed"));
            given += 1;
        }
        Ok(given)
    };

    rows.each("get-key", OBJECTS as u64, |round, d| {
        if let Some(p) = get(random_id(round))? {
            d.person(&p);
        }
        Ok::<_, redb::StorageError>(())
    });
    rows.each("get-email", 20_000, |round, d| {
        let address = format!("{}@example.com", random_number(round));

        if let Some(id) = email.get(address.as_str())? {
            d.person(&get(id.value() as i64)?.expect("indexed"));
        }
        Ok::<_, redb::StorageError>(())
    });
    rows.each("age-equal", 200, |round, d| {
        of_age((round % 80) as i64, usize::MAX, d).map(drop)
    });
    rows.each("age-range", 5_000, |round, d| {
        let low = (round % 76) as i64;
        let mut left = 20;

        for age in (low..=low + 4).rev() {
            left -= of_age(age, left, d)?;

            if left == 0 {
                break;
            }
        }
        Ok::<_, redb::StorageError>(())
    });
    rows.each("count", 200, |_, d| {
        let low = record::age_key(40, 0);

        d.number(ages.range(&low[..]..)?.count() as u64);
        Ok::<_, redb::StorageError>(())
    });
    rows.each("city-scan", 10, |round, d| {
        let city = format!("city {}", round % 100);

        for entry in people.iter()? {
            let (key, bytes) = entry?;

            if record::city(bytes.value()) == city.as_bytes() {
                d.person(&record::decode(key.value() as i64, bytes.value()));
            }
        }
        Ok::<_, redb::StorageError>(())
    });
    rows.each("top-score", 10, |_, d| {
        let mut top = Top::new();

        for entry in people.iter()? {
            let (key, bytes) = entry?;

            top.offer(record::score(bytes.value()), key.value() as i64);
        }

        for (_, id) in top.entries {
            d.person(&get(id)?.expect("present"));
        }
        Ok::<_, redb::StorageError>(())
    });
    drop((people, email, ages, txn));

    rows.all("update", 10_000, |_| {
        let txn = db.begin_write()?;
        let mut tables = Tables::open(&txn)?;

        for round in 0..10_000 {
            let id = random_id(round);
            let Some(mut p) = tables
                .people
                .get(id as u64)?
                .map(|bytes| record::decode(id, bytes.value()))
            else {
                continue;
            };

            tables.age.remove(&record::age_key(p.age, id)[..])?;
            p.age = (id + 1) % 80;
            record::encode(&p, &mut buffer);
            tables.people.insert(id as u64, buffer.as_slice())?;
            tables.age.insert(&record::age_key(p.age, id)[..], ())?;
        }

        drop(tables);
        txn.commit()?;
        Outcome::Ok(())
    });
    rows.all("delete", 10_000, |d| {
        let txn = db.begin_write()?;
        let mut tables = Tables::open(&txn)?;

        for round in 0..10_000 {
            let id = 1 + round * 7;
            let Some(p) = tables
                .people
                .remove(id as u64)?
                .map(|bytes| record::decode(id, bytes.value()))
            else {
                continue;
            };

            tables.email.remove(p.email.as_str())?;
            tables.age.remove(&record::age_key(p.age, id)[..])?;
            d.number(1);
        }

        drop(tables);
        txn.commit()?;
        Outcome::Ok(())
    });

    let txn = db.begin_read()?;
    let mut left = Digest::default();

    for entry in txn.open_table(PEOPLE)?.iter()? {
        let (key, bytes) = entry?;

        left.person(&record::decode(key.value() as i64, bytes.value()));
    }

    rows.check("left", left);
    Ok(())
}
