//! LMDB through heed. A sync commit is LMDB's default commit; a deferred one
//! runs with `NO_SYNC`, and the environment is synced once at the end, as
//! closing a DaruDB file syncs its deferred commits. The unique and age
//! indexes are tables of their own, written by hand as an application would.

use std::ops::Bound;
use std::path::Path;

use heed::types::Bytes;
use heed::{Database, Env, EnvFlags, EnvOpenOptions, FlagSetMode, RoTxn, RwTxn};

use crate::work::{Digest, OBJECTS, Person, Rows, Top, person, random_id, random_number, record};

struct Store {
    env: Env,
    people: Database<Bytes, Bytes>,
    email: Database<Bytes, Bytes>,
    age: Database<Bytes, Bytes>,
}

fn open(path: &Path) -> heed::Result<Store> {
    std::fs::create_dir_all(path)?;

    // SAFETY: one environment per path in this process.
    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(1 << 30)
            .max_dbs(4)
            .open(path)?
    };
    let mut txn = env.write_txn()?;
    let people = env.create_database(&mut txn, Some("people"))?;
    let email = env.create_database(&mut txn, Some("email"))?;
    let age = env.create_database(&mut txn, Some("age"))?;

    txn.commit()?;
    Ok(Store {
        env,
        people,
        email,
        age,
    })
}

impl Store {
    fn insert(&self, txn: &mut RwTxn<'_>, p: &Person, buffer: &mut Vec<u8>) -> heed::Result<()> {
        let id = self
            .people
            .last(txn)?
            .map_or(1, |(key, _)| record::id_of(key) + 1);

        record::encode(p, buffer);
        self.people.put(txn, &record::id_key(id), buffer)?;
        self.email
            .put(txn, p.email.as_bytes(), &record::id_key(id))?;
        self.age.put(txn, &record::age_key(p.age, id), &[])
    }

    fn get(&self, txn: &RoTxn<'_>, id: i64) -> heed::Result<Option<Person>> {
        Ok(self
            .people
            .get(txn, &record::id_key(id))?
            .map(|bytes| record::decode(id, bytes)))
    }

    /// The objects of one age, in key order, up to `limit`.
    fn of_age(
        &self,
        txn: &RoTxn<'_>,
        age: i64,
        limit: usize,
        d: &mut Digest,
    ) -> heed::Result<usize> {
        let low = record::age_key(age, 0);
        let high = record::age_key(age + 1, 0);
        let range = (Bound::Included(&low[..]), Bound::Excluded(&high[..]));
        let mut given = 0;

        for entry in self.age.range(txn, &range)? {
            if given == limit {
                break;
            }

            let (key, _) = entry?;

            d.person(&self.get(txn, record::id_of(key))?.expect("indexed"));
            given += 1;
        }
        Ok(given)
    }
}

pub fn run(directory: &Path, rows: &mut Rows) -> heed::Result<()> {
    let mut buffer = Vec::new();
    let store = open(&directory.join("commits.lmdb"))?;

    rows.each("insert-sync", 500, |round, _| {
        let mut txn = store.env.write_txn()?;

        store.insert(&mut txn, &person(round as i64), &mut buffer)?;
        txn.commit()
    });
    // SAFETY: commits that a power cut may undo, which is what a deferred
    // commit is.
    unsafe {
        store
            .env
            .set_flags(EnvFlags::NO_SYNC, FlagSetMode::Enable)?
    };
    rows.each("insert-deferred", 10_000, |round, _| {
        let mut txn = store.env.write_txn()?;

        store.insert(&mut txn, &person(1_000 + round as i64), &mut buffer)?;
        txn.commit()
    });
    store.env.force_sync()?;
    store.env.prepare_for_closing().wait();

    let store = open(&directory.join("objects.lmdb"))?;

    rows.all("insert-bulk", OBJECTS as u64, |_| {
        let mut txn = store.env.write_txn()?;

        for n in 0..OBJECTS {
            store.insert(&mut txn, &person(n), &mut buffer)?;
        }

        txn.commit()
    });

    let txn = store.env.read_txn()?;

    rows.each("get-key", OBJECTS as u64, |round, d| {
        if let Some(p) = store.get(&txn, random_id(round))? {
            d.person(&p);
        }
        Ok::<_, heed::Error>(())
    });
    rows.each("get-email", 20_000, |round, d| {
        let email = format!("{}@example.com", random_number(round));

        if let Some(id) = store.email.get(&txn, email.as_bytes())? {
            d.person(&store.get(&txn, record::id_of(id))?.expect("indexed"));
        }
        Ok::<_, heed::Error>(())
    });
    rows.each("age-equal", 200, |round, d| {
        store
            .of_age(&txn, (round % 80) as i64, usize::MAX, d)
            .map(drop)
    });
    rows.each("age-range", 5_000, |round, d| {
        let low = (round % 76) as i64;
        let mut left = 20;

        for age in (low..=low + 4).rev() {
            left -= store.of_age(&txn, age, left, d)?;

            if left == 0 {
                break;
            }
        }
        Ok::<_, heed::Error>(())
    });
    rows.each("count", 200, |_, d| {
        let low = record::age_key(40, 0);
        let range = (Bound::Included(&low[..]), Bound::Unbounded);

        d.number(store.age.range(&txn, &range)?.count() as u64);
        Ok::<_, heed::Error>(())
    });
    rows.each("city-scan", 10, |round, d| {
        let city = format!("city {}", round % 100);

        for entry in store.people.iter(&txn)? {
            let (key, bytes) = entry?;

            if record::city(bytes) == city.as_bytes() {
                d.person(&record::decode(record::id_of(key), bytes));
            }
        }
        Ok::<_, heed::Error>(())
    });
    rows.each("top-score", 10, |_, d| {
        let mut top = Top::new();

        for entry in store.people.iter(&txn)? {
            let (key, bytes) = entry?;

            top.offer(record::score(bytes), record::id_of(key));
        }

        for (_, id) in top.entries {
            d.person(&store.get(&txn, id)?.expect("present"));
        }
        Ok::<_, heed::Error>(())
    });
    drop(txn);

    rows.all("update", 10_000, |_| {
        let mut txn = store.env.write_txn()?;

        for round in 0..10_000 {
            let id = random_id(round);
            let Some(mut p) = store
                .people
                .get(&txn, &record::id_key(id))?
                .map(|bytes| record::decode(id, bytes))
            else {
                continue;
            };

            store.age.delete(&mut txn, &record::age_key(p.age, id))?;
            p.age = (id + 1) % 80;
            record::encode(&p, &mut buffer);
            store.people.put(&mut txn, &record::id_key(id), &buffer)?;
            store.age.put(&mut txn, &record::age_key(p.age, id), &[])?;
        }

        txn.commit()
    });
    rows.all("delete", 10_000, |d| {
        let mut txn = store.env.write_txn()?;

        for round in 0..10_000 {
            let id = 1 + round * 7;
            let Some(p) = store
                .people
                .get(&txn, &record::id_key(id))?
                .map(|bytes| record::decode(id, bytes))
            else {
                continue;
            };

            store.people.delete(&mut txn, &record::id_key(id))?;
            store.email.delete(&mut txn, p.email.as_bytes())?;
            store.age.delete(&mut txn, &record::age_key(p.age, id))?;
            d.number(1);
        }

        txn.commit()
    });

    let txn = store.env.read_txn()?;
    let mut left = Digest::default();

    for entry in store.people.iter(&txn)? {
        let (key, bytes) = entry?;

        left.person(&record::decode(record::id_of(key), bytes));
    }

    rows.check("left", left);
    Ok(())
}
