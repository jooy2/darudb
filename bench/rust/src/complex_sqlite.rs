//! The complex suite on SQLite through rusqlite, with the settings of the
//! simple suite: a write-ahead log with `synchronous = FULL`, `fullfsync`,
//! and a 32 MiB page cache. The schema is what an SQL application would
//! write: a foreign key for each link, the address's fields as columns of
//! their own, and a table for each list, with an index on every column the
//! other stores index. A row is read with its tags, through a subquery, so
//! that every store makes the same objects.

use std::path::Path;

use rusqlite::{Connection, Row, Statement, params};

use crate::complex::{self, BATCH, ORGANIZATIONS, PEOPLE, POSTS, Person, Post, rows, runs};
use crate::work::{Digest, Rows};

const PERSON: &str = "SELECT id, name, email, age, country, city, zip, score, active, joined_at, \
     organization, (SELECT group_concat(tag) FROM person_tags WHERE person = people.id) \
     FROM people";

const POST: &str = "SELECT id, author, title, body, likes, language, created_at, pinned, \
     (SELECT group_concat(tag) FROM post_tags WHERE post = posts.id) FROM posts";

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;

    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA fullfsync = ON;
         PRAGMA cache_size = -32768;
         CREATE TABLE organizations (
           code TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
           country TEXT NOT NULL, founded INTEGER NOT NULL);
         CREATE INDEX organizations_kind ON organizations (kind);
         CREATE INDEX organizations_country ON organizations (country);
         CREATE TABLE people (
           id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
           age INTEGER NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL,
           zip TEXT NOT NULL, score REAL NOT NULL, active INTEGER NOT NULL,
           joined_at INTEGER NOT NULL,
           organization TEXT NOT NULL REFERENCES organizations (code));
         CREATE INDEX people_age ON people (age);
         CREATE INDEX people_country ON people (country);
         CREATE INDEX people_joined_at ON people (joined_at);
         CREATE INDEX people_organization ON people (organization);
         CREATE TABLE person_tags (person INTEGER NOT NULL, tag TEXT NOT NULL);
         CREATE INDEX person_tags_person ON person_tags (person);
         CREATE INDEX person_tags_tag ON person_tags (tag, person);
         CREATE TABLE posts (
           id INTEGER PRIMARY KEY, author INTEGER NOT NULL REFERENCES people (id),
           title TEXT NOT NULL, body TEXT NOT NULL, likes INTEGER NOT NULL,
           language TEXT NOT NULL, created_at INTEGER NOT NULL, pinned INTEGER NOT NULL);
         CREATE INDEX posts_author ON posts (author);
         CREATE INDEX posts_likes ON posts (likes);
         CREATE INDEX posts_language ON posts (language);
         CREATE INDEX posts_created_at ON posts (created_at);
         CREATE TABLE post_tags (post INTEGER NOT NULL, tag TEXT NOT NULL);
         CREATE INDEX post_tags_post ON post_tags (post);
         CREATE INDEX post_tags_tag ON post_tags (tag, post);",
    )?;
    Ok(connection)
}

fn tags(text: Option<String>) -> Vec<String> {
    text.map(|text| text.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn person(row: &Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get(0)?,
        name: row.get(1)?,
        email: row.get(2)?,
        age: row.get(3)?,
        country: row.get(4)?,
        city: row.get(5)?,
        zip: row.get(6)?,
        score: row.get(7)?,
        active: row.get(8)?,
        joined_at: row.get(9)?,
        organization: row.get(10)?,
        tags: tags(row.get(11)?),
    })
}

fn post(row: &Row<'_>) -> rusqlite::Result<Post> {
    Ok(Post {
        id: row.get(0)?,
        author: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        likes: row.get(4)?,
        language: row.get(5)?,
        created_at: row.get(6)?,
        pinned: row.get(7)?,
        tags: tags(row.get(8)?),
    })
}

fn people(
    statement: &mut Statement<'_>,
    parameters: impl rusqlite::Params,
    d: &mut Digest,
) -> rusqlite::Result<()> {
    let mut found = statement.query(parameters)?;

    while let Some(row) = found.next()? {
        let p = person(row)?;

        d.add(p.id, p.age);
    }
    Ok(())
}

fn posts(
    statement: &mut Statement<'_>,
    parameters: impl rusqlite::Params,
    d: &mut Digest,
) -> rusqlite::Result<()> {
    let mut found = statement.query(parameters)?;

    while let Some(row) = found.next()? {
        let p = post(row)?;

        d.add(p.id, p.likes);
    }
    Ok(())
}

fn count(
    statement: &mut Statement<'_>,
    parameters: impl rusqlite::Params,
    d: &mut Digest,
) -> rusqlite::Result<()> {
    let n: i64 = statement.query_row(parameters, |row| row.get(0))?;

    d.number(n as u64);
    Ok(())
}

pub fn run(directory: &Path, rows: &mut Rows) -> rusqlite::Result<()> {
    let connection = open(&directory.join("complex.sqlite"))?;

    // The objects are made before the load's time starts.
    let new_organizations: Vec<_> = (0..ORGANIZATIONS).map(complex::organization).collect();
    let new_people: Vec<_> = (0..PEOPLE).map(complex::person).collect();
    let new_posts: Vec<_> = (0..POSTS).map(complex::post).collect();

    rows.all(rows::LOAD.0, rows::LOAD.1, |_| {
        for batch in new_organizations.chunks(BATCH) {
            let txn = connection.unchecked_transaction()?;
            let mut insert = txn.prepare_cached(
                "INSERT INTO organizations (code, name, kind, country, founded) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;

            for o in batch {
                insert.execute(params![o.code, o.name, o.kind, o.country, o.founded])?;
            }

            drop(insert);
            txn.commit()?;
        }

        for batch in new_people.chunks(BATCH) {
            let txn = connection.unchecked_transaction()?;
            let mut insert = txn.prepare_cached(
                "INSERT INTO people (name, email, age, country, city, zip, score, active, \
                 joined_at, organization) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut tag =
                txn.prepare_cached("INSERT INTO person_tags (person, tag) VALUES (?1, ?2)")?;

            for p in batch {
                insert.execute(params![
                    p.name,
                    p.email,
                    p.age,
                    p.country,
                    p.city,
                    p.zip,
                    p.score,
                    p.active,
                    p.joined_at,
                    p.organization
                ])?;

                let id = txn.last_insert_rowid();

                for t in &p.tags {
                    tag.execute(params![id, t])?;
                }
            }

            drop((insert, tag));
            txn.commit()?;
        }

        for batch in new_posts.chunks(BATCH) {
            let txn = connection.unchecked_transaction()?;
            let mut insert = txn.prepare_cached(
                "INSERT INTO posts (author, title, body, likes, language, created_at, pinned) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let mut tag =
                txn.prepare_cached("INSERT INTO post_tags (post, tag) VALUES (?1, ?2)")?;

            for p in batch {
                insert.execute(params![
                    p.author,
                    p.title,
                    p.body,
                    p.likes,
                    p.language,
                    p.created_at,
                    p.pinned
                ])?;

                let id = txn.last_insert_rowid();

                for t in &p.tags {
                    tag.execute(params![id, t])?;
                }
            }

            drop((insert, tag));
            txn.commit()?;
        }

        Ok::<_, rusqlite::Error>(())
    });
    drop((new_organizations, new_people, new_posts));

    connection.execute_batch("BEGIN")?;
    {
        let mut by_id = connection.prepare_cached(&format!("{PERSON} WHERE id = ?1"))?;
        let mut organization = connection.prepare_cached(
            "SELECT code, name, kind, country, founded FROM organizations WHERE code = ?1",
        )?;
        let mut newest = connection.prepare_cached(&format!(
            "{POST} WHERE author = ?1 ORDER BY created_at DESC, id LIMIT 5"
        ))?;
        let mut filter = connection.prepare_cached(&format!(
            "{PERSON} WHERE age BETWEEN ?1 AND ?2 AND country = ?3 AND active = 1 \
             ORDER BY score DESC, id LIMIT 50"
        ))?;
        let mut either = connection
            .prepare_cached("SELECT count(*) FROM people WHERE country IN (?1, ?2) OR age >= 78")?;
        let mut prefix = connection.prepare_cached(&format!(
            "{PERSON} WHERE substr(name, 1, length(?1)) = ?1 ORDER BY id"
        ))?;
        let mut page = connection.prepare_cached(&format!(
            "{PERSON} ORDER BY joined_at, id LIMIT 50 OFFSET ?1"
        ))?;
        let mut author_country = connection.prepare_cached(&format!(
            "{POST} WHERE likes >= 900 AND author IN (SELECT id FROM people WHERE country = ?1) \
             ORDER BY created_at DESC, id LIMIT 20"
        ))?;
        let mut two_links = connection.prepare_cached(
            "SELECT count(*) FROM posts JOIN people ON people.id = posts.author \
             JOIN organizations ON organizations.code = people.organization \
             WHERE organizations.kind = ?1 AND posts.language = ?2",
        )?;
        let mut tag = connection.prepare_cached(&format!(
            "{POST} WHERE id IN (SELECT post FROM post_tags WHERE tag = ?1) \
             ORDER BY likes DESC, id LIMIT 20"
        ))?;
        let mut text =
            connection.prepare_cached("SELECT count(*) FROM posts WHERE instr(body, ?1) > 0")?;
        let mut likes = connection
            .prepare_cached("SELECT count(*) FROM posts WHERE likes >= 500 AND language = ?1")?;

        rows.each(rows::DETAIL.0, rows::DETAIL.1, |round, d| {
            let found = match by_id.query_row([runs::detail(round)], person) {
                Ok(found) => found,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
                Err(error) => return Err(error),
            };

            d.add(found.id, found.age);

            match organization.query_row([&found.organization], |row| {
                Ok(complex::Organization {
                    code: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    country: row.get(3)?,
                    founded: row.get(4)?,
                })
            }) {
                Ok(o) => d.add(o.founded, 0),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(error) => return Err(error),
            }

            posts(&mut newest, [found.id], d)
        });
        rows.each(rows::FILTER.0, rows::FILTER.1, |round, d| {
            let (age, country) = runs::filter(round);

            people(&mut filter, params![age, age + 9, country], d)
        });
        rows.each(rows::EITHER.0, rows::EITHER.1, |round, d| {
            let (first, second) = runs::either(round);

            count(&mut either, [first, second], d)
        });
        rows.each(rows::PREFIX.0, rows::PREFIX.1, |round, d| {
            people(&mut prefix, [runs::prefix(round)], d)
        });
        rows.each(rows::PAGE.0, rows::PAGE.1, |round, d| {
            people(&mut page, [runs::offset(round) as i64], d)
        });
        rows.each(
            rows::AUTHOR_COUNTRY.0,
            rows::AUTHOR_COUNTRY.1,
            |round, d| posts(&mut author_country, [runs::author_country(round)], d),
        );
        rows.each(rows::TWO_LINKS.0, rows::TWO_LINKS.1, |round, d| {
            let (kind, language) = runs::two_links(round);

            count(&mut two_links, [kind, language], d)
        });
        rows.each(rows::TAG.0, rows::TAG.1, |round, d| {
            posts(&mut tag, [runs::tag(round)], d)
        });
        rows.each(rows::TEXT.0, rows::TEXT.1, |round, d| {
            count(&mut text, [runs::phrase(round)], d)
        });
        rows.each(rows::COUNT.0, rows::COUNT.1, |round, d| {
            count(&mut likes, [runs::language(round)], d)
        });
    }
    connection.execute_batch("COMMIT")?;

    rows.all(rows::UPDATE, runs::updated(), |d| {
        let txn = connection.unchecked_transaction()?;
        let mut update =
            txn.prepare_cached("UPDATE people SET age = (age + 1) % 80 WHERE country = ?1")?;

        for country in runs::UPDATED {
            d.number(update.execute([country])? as u64);
        }

        drop(update);
        txn.commit()
    });
    rows.all(rows::DELETE, runs::DELETED, |d| {
        let txn = connection.unchecked_transaction()?;
        let mut post_tags = txn.prepare_cached(
            "DELETE FROM post_tags WHERE post IN (SELECT id FROM posts WHERE author = ?1)",
        )?;
        let mut posts = txn.prepare_cached("DELETE FROM posts WHERE author = ?1")?;
        let mut person_tags = txn.prepare_cached("DELETE FROM person_tags WHERE person = ?1")?;
        let mut person = txn.prepare_cached("DELETE FROM people WHERE id = ?1")?;

        for round in 0..runs::DELETED {
            let id = runs::deleted(round);

            post_tags.execute([id])?;

            let deleted = posts.execute([id])?;

            person_tags.execute([id])?;

            if person.execute([id])? == 1 {
                d.add(id, deleted as i64);
            }
        }

        drop((post_tags, posts, person_tags, person));
        txn.commit()
    });

    let mut left_people = Digest::default();
    let mut left_posts = Digest::default();

    people(
        &mut connection.prepare(&format!("{PERSON} ORDER BY id"))?,
        [],
        &mut left_people,
    )?;
    posts(
        &mut connection.prepare(&format!("{POST} ORDER BY id"))?,
        [],
        &mut left_posts,
    )?;
    rows.check("left-people", left_people);
    rows.check("left-posts", left_posts);
    Ok(())
}
