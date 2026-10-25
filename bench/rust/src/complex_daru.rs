//! The complex suite on DaruDB through its Rust API, with each collection's
//! type derived, as a Rust application would declare them: a link from each
//! person to an organization and from each post to its author, an embedded
//! address, and lists of tags with an index on each.

use std::path::Path;

use darudb::{Collection, Link, OpenOptions, Query, Schema, Value};

use crate::complex::{self, BATCH, ORGANIZATIONS, PEOPLE, POSTS, rows, runs};
use crate::work::{Digest, Rows};

#[derive(darudb::Object, Debug)]
#[darudb(collection = "organizations")]
struct Organization {
    #[darudb(key)]
    code: String,
    name: String,
    #[darudb(index)]
    kind: String,
    #[darudb(index)]
    country: String,
    founded: i64,
}

#[derive(darudb::Embedded, Debug)]
struct Address {
    city: String,
    zip: String,
}

#[derive(darudb::Object, Debug)]
#[darudb(collection = "people")]
struct Person {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: String,
    #[darudb(index)]
    age: i64,
    #[darudb(index)]
    country: String,
    address: Address,
    score: f64,
    active: bool,
    #[darudb(index)]
    joined_at: i64,
    #[darudb(index)]
    organization: Link<Organization>,
    #[darudb(index)]
    tags: Vec<String>,
}

#[derive(darudb::Object, Debug)]
#[darudb(collection = "posts")]
struct Post {
    id: Option<i64>,
    #[darudb(index)]
    author: Link<Person>,
    title: String,
    body: String,
    #[darudb(index)]
    likes: i64,
    #[darudb(index)]
    language: String,
    #[darudb(index)]
    created_at: i64,
    pinned: bool,
    #[darudb(index)]
    tags: Vec<String>,
}

fn organization(n: i64) -> Organization {
    let o = complex::organization(n);

    Organization {
        code: o.code,
        name: o.name,
        kind: o.kind,
        country: o.country,
        founded: o.founded,
    }
}

fn person(n: i64) -> Person {
    let p = complex::person(n);

    Person {
        id: None,
        name: p.name,
        email: p.email,
        age: p.age,
        country: p.country,
        address: Address {
            city: p.city,
            zip: p.zip,
        },
        score: p.score,
        active: p.active,
        joined_at: p.joined_at,
        organization: Link::new(p.organization),
        tags: p.tags,
    }
}

fn post(m: i64) -> Post {
    let p = complex::post(m);

    Post {
        id: None,
        author: Link::new(p.author),
        title: p.title,
        body: p.body,
        likes: p.likes,
        language: p.language,
        created_at: p.created_at,
        pinned: p.pinned,
        tags: p.tags,
    }
}

fn person_digest(d: &mut Digest, person: &Person) {
    d.add(person.id.expect("an id"), person.age);
}

fn post_digest(d: &mut Digest, post: &Post) {
    d.add(post.id.expect("an id"), post.likes);
}

fn bind(query: &Query, values: impl IntoIterator<Item = Value>) -> darudb::Result<Query> {
    query.bind(&values.into_iter().collect::<Vec<_>>())
}

pub fn run(directory: &Path, rows: &mut Rows) -> darudb::Result<()> {
    let mut options = OpenOptions::new();

    options.schema(
        Schema::new(1)
            .collection(Collection::of::<Organization>())
            .collection(Collection::of::<Person>())
            .collection(Collection::of::<Post>()),
    );

    let db = options.open(directory.join("complex.darudb"))?;

    // The objects are made before the load's time starts.
    let new_organizations: Vec<_> = (0..ORGANIZATIONS).map(organization).collect();
    let new_people: Vec<_> = (0..PEOPLE).map(person).collect();
    let new_posts: Vec<_> = (0..POSTS).map(post).collect();

    rows.all(rows::LOAD.0, rows::LOAD.1, |_| {
        for batch in new_organizations.chunks(BATCH) {
            let mut txn = db.begin_write()?;

            let mut collection = txn.collection_of::<Organization>()?;

            for object in batch {
                collection.insert(object)?;
            }

            drop(collection);
            txn.commit()?;
        }

        for batch in new_people.chunks(BATCH) {
            let mut txn = db.begin_write()?;

            let mut collection = txn.collection_of::<Person>()?;

            for object in batch {
                collection.insert(object)?;
            }

            drop(collection);
            txn.commit()?;
        }

        for batch in new_posts.chunks(BATCH) {
            let mut txn = db.begin_write()?;

            let mut collection = txn.collection_of::<Post>()?;

            for object in batch {
                collection.insert(object)?;
            }

            drop(collection);
            txn.commit()?;
        }

        Ok::<_, darudb::Error>(())
    });
    drop((new_organizations, new_people, new_posts));

    let read = db.begin_read()?;
    let organizations = read.collection_of::<Organization>()?;
    let people = read.collection_of::<Person>()?;
    let posts = read.collection_of::<Post>()?;

    let newest = Query::prepare("author == $0 SORT BY created_at DESC LIMIT 5")?;
    let filter = Query::prepare(
        "age BETWEEN $0 AND $1 AND country == $2 AND active == true SORT BY score DESC LIMIT 50",
    )?;
    let either = Query::prepare("country IN [$0, $1] OR age >= 78")?;
    let prefix = Query::prepare("name STARTSWITH $0")?;
    let author_country =
        Query::prepare("author.country == $0 AND likes >= 900 SORT BY created_at DESC LIMIT 20")?;
    let two_links = Query::prepare("author.organization.kind == $0 AND language == $1")?;
    let tag = Query::prepare("tags CONTAINS $0 SORT BY likes DESC LIMIT 20")?;
    let text = Query::prepare("body CONTAINS $0")?;
    let count = Query::prepare("likes >= 500 AND language == $0")?;

    rows.each(rows::DETAIL.0, rows::DETAIL.1, |round, d| {
        let Some(person) = people.get(runs::detail(round))? else {
            return Ok(());
        };

        person_digest(d, &person);

        if let Some(organization) = organizations.get(person.organization.key().clone())? {
            d.add(organization.founded, 0);
        }

        for post in posts.query(&bind(&newest, [person.id.expect("an id").into()])?)? {
            post_digest(d, &post);
        }

        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::FILTER.0, rows::FILTER.1, |round, d| {
        let (age, country) = runs::filter(round);

        for person in people.query(&bind(
            &filter,
            [age.into(), (age + 9).into(), country.into()],
        )?)? {
            person_digest(d, &person);
        }

        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::EITHER.0, rows::EITHER.1, |round, d| {
        let (first, second) = runs::either(round);

        d.number(people.count(&bind(&either, [first.into(), second.into()])?)?);
        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::PREFIX.0, rows::PREFIX.1, |round, d| {
        for person in people.query(&bind(&prefix, [runs::prefix(round).into()])?)? {
            person_digest(d, &person);
        }

        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::PAGE.0, rows::PAGE.1, |round, d| {
        let page = Query::new()
            .sort_by("joined_at")
            .offset(runs::offset(round))
            .limit(50);

        for person in people.query(&page)? {
            person_digest(d, &person);
        }

        Ok::<_, darudb::Error>(())
    });
    rows.each(
        rows::AUTHOR_COUNTRY.0,
        rows::AUTHOR_COUNTRY.1,
        |round, d| {
            for post in posts.query(&bind(
                &author_country,
                [runs::author_country(round).into()],
            )?)? {
                post_digest(d, &post);
            }

            Ok::<_, darudb::Error>(())
        },
    );
    rows.each(rows::TWO_LINKS.0, rows::TWO_LINKS.1, |round, d| {
        let (kind, language) = runs::two_links(round);

        d.number(posts.count(&bind(&two_links, [kind.into(), language.into()])?)?);
        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::TAG.0, rows::TAG.1, |round, d| {
        for post in posts.query(&bind(&tag, [runs::tag(round).into()])?)? {
            post_digest(d, &post);
        }

        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::TEXT.0, rows::TEXT.1, |round, d| {
        d.number(posts.count(&bind(&text, [runs::phrase(round).into()])?)?);
        Ok::<_, darudb::Error>(())
    });
    rows.each(rows::COUNT.0, rows::COUNT.1, |round, d| {
        d.number(posts.count(&bind(&count, [runs::language(round).into()])?)?);
        Ok::<_, darudb::Error>(())
    });
    drop((organizations, people, posts));
    drop(read);

    let in_country = Query::prepare("country == $0")?;
    let by_author = Query::prepare("author == $0")?;

    rows.all(rows::UPDATE, runs::updated(), |d| {
        let mut txn = db.begin_write()?;
        let mut people = txn.collection_of::<Person>()?;

        for country in runs::UPDATED {
            let found = people.query(&bind(&in_country, [country.into()])?)?;

            d.number(found.len() as u64);

            for person in found {
                people.set(
                    person.id.expect("an id"),
                    Person::AGE,
                    (person.age + 1) % 80,
                )?;
            }
        }

        drop(people);
        txn.commit()
    });

    rows.all(rows::DELETE, runs::DELETED, |d| {
        let mut txn = db.begin_write()?;

        for round in 0..runs::DELETED {
            let id = runs::deleted(round);
            let mut posts = txn.collection_of::<Post>()?;
            let mut deleted = 0;

            for post in posts.query(&bind(&by_author, [id.into()])?)? {
                if posts.delete(post.id.expect("an id"))? {
                    deleted += 1;
                }
            }

            drop(posts);

            if txn.collection_of::<Person>()?.delete(id)? {
                d.add(id, deleted);
            }
        }

        txn.commit()
    });

    let read = db.begin_read()?;
    let mut people = Digest::default();
    let mut posts = Digest::default();

    for person in read.collection_of::<Person>()?.iter()? {
        person_digest(&mut people, &person?);
    }

    for post in read.collection_of::<Post>()?.iter()? {
        post_digest(&mut posts, &post?);
    }

    rows.check("left-people", people);
    rows.check("left-posts", posts);
    drop(read);
    db.close()
}
