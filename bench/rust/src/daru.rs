//! DaruDB through its Rust API, with the collection's type derived:
//! `collection_of::<Person>()` for every read and write, so that records are
//! read straight into the struct, as a Rust application would use it.

use std::path::Path;

use darudb::{Collection, Filter, OpenOptions, Query, Schema};

use crate::work::{self, Digest, OBJECTS, Rows, random_id, random_number};

#[derive(darudb::Object, Debug)]
#[darudb(collection = "people")]
struct Person {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: String,
    #[darudb(index)]
    age: i64,
    city: String,
    score: f64,
}

fn person(n: i64) -> Person {
    let p = work::person(n);

    Person {
        id: None,
        name: p.name,
        email: p.email,
        age: p.age,
        city: p.city,
        score: p.score,
    }
}

fn digest(digest: &mut Digest, object: &Person) {
    digest.add(object.id.expect("an id"), object.age);
}

pub fn run(directory: &Path, rows: &mut Rows) -> darudb::Result<()> {
    let mut options = OpenOptions::new();

    options.schema(Schema::new(1).collection(Collection::of::<Person>()));

    let db = options.open(directory.join("commits.darudb"))?;

    rows.each("insert-sync", 500, |round, _| {
        let mut txn = db.begin_write()?;

        txn.collection_of::<Person>()?
            .insert(&person(round as i64))?;
        txn.commit()
    });
    rows.each("insert-deferred", 10_000, |round, _| {
        let mut txn = db.begin_write()?;

        txn.collection_of::<Person>()?
            .insert(&person(1_000 + round as i64))?;
        txn.commit_deferred()
    });
    db.close()?;

    let db = options.open(directory.join("objects.darudb"))?;

    rows.all("insert-bulk", OBJECTS as u64, |_| {
        let mut txn = db.begin_write()?;
        let mut people = txn.collection_of::<Person>()?;

        for n in 0..OBJECTS {
            people.insert(&person(n))?;
        }

        drop(people);
        txn.commit()
    });

    let read = db.begin_read()?;
    let people = read.collection_of::<Person>()?;
    let by_email = Query::prepare("email == $0")?;
    let by_age = Query::prepare("age == $0")?;
    let range = Query::prepare("age BETWEEN $0 AND $1 SORT BY age DESC LIMIT 20")?;
    let at_least = Query::new().filter(Filter::ge("age", 40));
    let in_city = Query::prepare("city == $0")?;
    let top = Query::new().sort_by_desc("score").limit(10);

    rows.each("get-key", OBJECTS as u64, |round, d| {
        if let Some(object) = people.get(random_id(round))? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    rows.each("get-email", 20_000, |round, d| {
        let email = format!("{}@example.com", random_number(round));

        for object in people.query(&by_email.bind(&[email.into()])?)? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    rows.each("age-equal", 200, |round, d| {
        for object in people.query(&by_age.bind(&[((round % 80) as i64).into()])?)? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    rows.each("age-range", 5_000, |round, d| {
        let age = (round % 76) as i64;

        for object in people.query(&range.bind(&[age.into(), (age + 4).into()])?)? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    rows.each("count", 200, |_, d| {
        d.number(people.count(&at_least)?);
        Ok::<_, darudb::Error>(())
    });
    rows.each("city-scan", 10, |round, d| {
        let city = format!("city {}", round % 100);

        for object in people.query(&in_city.bind(&[city.into()])?)? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    rows.each("top-score", 10, |_, d| {
        for object in people.query(&top)? {
            digest(d, &object);
        }
        Ok::<_, darudb::Error>(())
    });
    drop(people);
    drop(read);

    rows.all("update", 10_000, |_| {
        let mut txn = db.begin_write()?;
        let mut people = txn.collection_of::<Person>()?;

        for round in 0..10_000 {
            let id = random_id(round);

            if let Some(mut person) = people.get(id)? {
                person.age = (id + 1) % 80;
                people.put(&person)?;
            }
        }

        drop(people);
        txn.commit()
    });
    rows.all("delete", 10_000, |d| {
        let mut txn = db.begin_write()?;
        let mut people = txn.collection_of::<Person>()?;

        for round in 0..10_000 {
            if people.delete(1 + round * 7)? {
                d.number(1);
            }
        }

        drop(people);
        txn.commit()
    });

    let read = db.begin_read()?;
    let mut left = Digest::default();

    for object in read.collection_of::<Person>()?.iter()? {
        digest(&mut left, &object?);
    }

    rows.check("left", left);
    drop(read);
    db.close()
}
