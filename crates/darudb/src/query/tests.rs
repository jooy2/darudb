//! Every query gives the scan's answer: random queries over random data, run
//! through the planner and as a walk of every record, return the same
//! objects in the same order, and count the same.

use super::build::{Filter, Query};
use super::ir::{self, Ir};
use super::{plan, run};
use crate::format::object::{Object, Value};
use crate::schema::objects::Source;
use crate::schema::{Collection, Embedded, Schema, Type};
use crate::testing::Rng;
use crate::{Database, OpenOptions};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("teams")
                .primary_key("name", Type::String)
                .optional("city", Type::String)
                .index("city"),
        )
        .collection(
            Collection::new("players")
                .with_default("score", Type::Int, 0)
                .optional("rating", Type::Float)
                .optional("handle", Type::String)
                .optional("tags", Type::list(Type::String))
                .optional("team", Type::link("teams"))
                .optional("friends", Type::list(Type::link("players")))
                .with_default("active", Type::Bool, true)
                .optional(
                    "address",
                    Type::object(
                        Embedded::new()
                            .optional("city", Type::String)
                            .optional("zip", Type::Int),
                    ),
                )
                .optional("note", Type::Bytes)
                .index("score")
                .index("rating")
                .unique("handle")
                .index("tags")
                .index("team")
                .index("friends")
                .index("active"),
        )
}

const HANDLES: [&str; 8] = ["ace", "bee", "bean", "cat", "", "a\0b", "a", "é"];
const TAGS: [&str; 4] = ["red", "blue", "green", "re"];
const TEAMS: [&str; 3] = ["north", "south", "gone"];
const CITIES: [&str; 3] = ["Seoul", "Busan", "Seo"];
const RATINGS: [f64; 7] = [-1.0, -0.0, 0.0, 0.5, 2.0, f64::INFINITY, f64::NAN];

fn pick<T: Copy>(rng: &mut Rng, of: &[T]) -> T {
    of[rng.index(of.len())]
}

fn int(rng: &mut Rng, below: u64) -> i64 {
    i64::try_from(rng.below(below)).unwrap()
}

fn player(rng: &mut Rng) -> Object {
    let mut object = Object::new();

    if rng.below(4) > 0 {
        object.set("score", int(rng, 7) - 3);
    }

    if rng.below(3) > 0 {
        object.set("rating", pick(rng, &RATINGS));
    }

    if rng.below(2) > 0 {
        object.set("handle", pick(rng, &HANDLES));
    }

    if rng.below(2) > 0 {
        let tags: Vec<Value> = (0..rng.below(4)).map(|_| pick(rng, &TAGS).into()).collect();

        object.set("tags", tags);
    }

    if rng.below(3) > 0 {
        object.set("team", pick(rng, &TEAMS));
    }

    if rng.below(3) == 0 {
        let friends: Vec<Value> = (0..rng.below(3))
            .map(|_| (1 + int(rng, 30)).into())
            .collect();

        object.set("friends", friends);
    }

    if rng.below(3) > 0 {
        object.set("active", rng.below(3) > 0);
    }

    if rng.below(2) > 0 {
        let mut address = Object::new();

        if rng.below(3) > 0 {
            address.set("city", pick(rng, &CITIES));
        }

        if rng.below(3) > 0 {
            address.set("zip", int(rng, 4));
        }

        object.set("address", address);
    }

    if rng.below(3) == 0 {
        let len = rng.index(3);

        object.set("note", rng.bytes(len));
    }

    object
}

/// A random value of the type at `path`, from the values the data holds and
/// a few it does not.
fn value(rng: &mut Rng, path: &str) -> Value {
    match path {
        "id" | "friends" => (int(rng, 32) - 1).into(),
        "score" | "friends.score" => (int(rng, 9) - 4).into(),
        "rating" => pick(rng, &[-1.0, -0.0, 0.5, 1.0, f64::INFINITY, f64::NAN]).into(),
        "handle" => pick(rng, &["ace", "bea", "be", "", "a", "a\0", "z"]).into(),
        "tags" => pick(rng, &TAGS).into(),
        "team" => pick(rng, &TEAMS).into(),
        "team.city" | "address.city" => pick(rng, &["Seoul", "Busan", "Seo", "S", "x"]).into(),
        "active" => (rng.below(2) == 1).into(),
        "address.zip" => (int(rng, 5) - 1).into(),
        _ => {
            let len = rng.index(3);

            rng.bytes(len).into()
        }
    }
}

const PATHS: [&str; 13] = [
    "id",
    "score",
    "rating",
    "handle",
    "tags",
    "team",
    "team.city",
    "friends",
    "friends.score",
    "active",
    "address.city",
    "address.zip",
    "note",
];

fn is_string(path: &str) -> bool {
    matches!(
        path,
        "handle" | "tags" | "team" | "team.city" | "address.city"
    )
}

fn test(rng: &mut Rng) -> Filter {
    let path = pick(rng, &PATHS);

    match rng.below(12) {
        0 => Filter::eq(path, value(rng, path)),
        1 => Filter::ne(path, value(rng, path)),
        2 => Filter::lt(path, value(rng, path)),
        3 => Filter::le(path, value(rng, path)),
        4 => Filter::gt(path, value(rng, path)),
        5 => Filter::ge(path, value(rng, path)),
        6 => {
            let (low, high) = (value(rng, path), value(rng, path));

            Filter::between(path, low, high)
        }
        7 => {
            let values: Vec<Value> = (0..rng.below(4)).map(|_| value(rng, path)).collect();

            Filter::is_in(path, values)
        }
        8 if is_string(path) || path == "friends" => Filter::contains(path, value(rng, path)),
        9 if is_string(path) => Filter::starts_with(path, value(rng, path)),
        10 if is_string(path) => Filter::ends_with(path, value(rng, path)),
        11 => Filter::is_null(path),
        _ => Filter::is_not_null(path),
    }
}

fn filter(rng: &mut Rng, depth: u32) -> Filter {
    if depth == 0 || rng.below(3) == 0 {
        return test(rng);
    }

    match rng.below(4) {
        0 => filter(rng, depth - 1).or(filter(rng, depth - 1)),
        1 => !filter(rng, depth - 1),
        _ => filter(rng, depth - 1).and(filter(rng, depth - 1)),
    }
}

const SORTABLE: [&str; 9] = [
    "id",
    "score",
    "rating",
    "handle",
    "team",
    "team.city",
    "active",
    "address.zip",
    "note",
];

fn query(rng: &mut Rng) -> Query {
    let mut query = Query::new();

    // Mostly a top-level `AND` of plain tests, which the planner reads.
    for _ in 0..rng.below(4) {
        query = query.filter(if rng.below(4) == 0 {
            filter(rng, 2)
        } else {
            test(rng)
        });
    }

    for _ in 0..rng.below(3) {
        let path = pick(rng, &SORTABLE);

        query = if rng.below(2) == 0 {
            query.sort_by(path)
        } else {
            query.sort_by_desc(path)
        };
    }

    if rng.below(3) == 0 {
        query = query.offset(rng.below(6));
    }

    if rng.below(2) == 0 {
        query = query.limit(rng.below(8));
    }

    query
}

/// The ids of `objects`, which compare where objects holding NaN would not.
fn ids(objects: &[Object]) -> Vec<i64> {
    objects
        .iter()
        .map(|object| object.get("id").and_then(Value::as_int).unwrap())
        .collect()
}

/// Runs `ir` through the planner and as a scan, and checks the two agree.
/// Returns the planned objects' ids, or `None` for an invalid query.
fn agree(
    source: &dyn Source,
    db_schema: &crate::format::object::schema::StoredSchema,
    ir: &Ir,
) -> Option<Vec<i64>> {
    let collection = db_schema.collection("players").unwrap();
    let planned = plan::plan(db_schema, collection, ir);
    let scanned = plan::scan(db_schema, collection, ir);

    let (planned, scanned) = match (planned, scanned) {
        (Ok(planned), Ok(scanned)) => (planned, scanned),
        (Err(a), Err(b)) => {
            assert_eq!((a.code(), b.code()), ("INVALID_QUERY", "INVALID_QUERY"));

            return None;
        }
        (a, b) => panic!(
            "{ir:?}: the plan gives {:?} and the scan {:?}",
            a.err(),
            b.err()
        ),
    };
    let found = ids(&run::objects(source, &planned).unwrap());
    let expected = ids(&run::objects(source, &scanned).unwrap());

    assert_eq!(found, expected, "{ir:?}\nplanned {planned:?}");
    assert_eq!(
        run::count(source, &planned).unwrap(),
        run::count(source, &scanned).unwrap(),
        "{ir:?}\nplanned {planned:?}"
    );
    assert_eq!(
        run::count(source, &planned).unwrap(),
        u64::try_from(found.len()).unwrap(),
        "{ir:?}"
    );

    Some(found)
}

#[test]
fn every_query_gives_the_scans_answer() {
    for seed in 0..6 {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        if seed % 2 == 1 {
            options.key([5; 32]);
        }

        let db = options.open(dir.path().join("query.darudb")).unwrap();
        let mut rng = Rng::new(seed);
        let mut txn = db.begin_write().unwrap();

        for (name, city) in [("north", Some("Seoul")), ("south", None)] {
            let mut team = Object::new().with("name", name);

            if let Some(city) = city {
                team.set("city", city);
            }

            txn.collection("teams").unwrap().insert(team).unwrap();
        }

        for _ in 0..30 {
            let mut players = txn.collection("players").unwrap();
            let mut object = player(&mut rng);

            // A handle already taken is left out, since it is unique.
            if players.insert(object.clone()).is_err() {
                object.remove("handle");
                players.insert(object).unwrap();
            }
        }

        txn.commit().unwrap();

        let mut valid = 0;
        let mut nonempty = 0;
        let read = db.begin_read().unwrap();
        let open = read.schema().cloned().unwrap();

        for _ in 0..1500 {
            let query = query(&mut rng);

            // The IR a query crosses the language boundary as reads back as
            // the same query. Compared as bytes, since NaN is not equal to
            // itself as a value.
            let bytes = ir::encode("players", &query.ir, false).unwrap();
            let (_, decoded, _) = ir::decode(&bytes).unwrap();

            assert_eq!(ir::encode("players", &decoded, false).unwrap(), bytes);

            if let Some(found) = agree(&read, &open.schema, &query.ir) {
                valid += 1;
                nonempty += usize::from(!found.is_empty());
            }
        }

        assert!(valid > 1000, "seed {seed}: only {valid} queries were valid");
        assert!(
            nonempty > 300,
            "seed {seed}: only {nonempty} queries found anything"
        );

        // A write transaction's own changes are queried the same way.
        let mut txn = db.begin_write().unwrap();

        for _ in 0..10 {
            let _ = txn
                .collection("players")
                .unwrap()
                .put(player(&mut rng).with("id", 1 + int(&mut rng, 40)));
        }

        for _ in 0..300 {
            let query = query(&mut rng);
            let open = txn.schema().cloned().unwrap();

            agree(&txn, &open.schema, &query.ir);
        }
    }
}

fn database(dir: &tempfile::TempDir) -> Database {
    let db = OpenOptions::new()
        .schema(schema())
        .open(dir.path().join("query.darudb"))
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("teams")
        .unwrap()
        .insert(Object::new().with("name", "north").with("city", "Seoul"))
        .unwrap();

    let mut players = txn.collection("players").unwrap();

    for object in [
        Object::new()
            .with("score", 3)
            .with("rating", 0.0)
            .with("handle", "ace")
            .with("tags", vec![Value::from("red"), Value::from("blue")])
            .with("team", "north"),
        Object::new()
            .with("score", 1)
            .with("rating", -0.0)
            .with("tags", Vec::<Value>::new())
            .with("team", "gone")
            .with("address", Object::new().with("city", "Busan")),
        Object::new()
            .with("score", 3)
            .with("rating", f64::NAN)
            .with("handle", "a\0b")
            .with("friends", vec![Value::from(1), Value::from(9)]),
        Object::new().with("active", false),
    ] {
        players.insert(object).unwrap();
    }

    txn.commit().unwrap();
    db
}

fn find(db: &Database, query: Query) -> Vec<i64> {
    let read = db.begin_read().unwrap();

    ids(&read.collection("players").unwrap().query(&query).unwrap())
}

#[test]
fn queries_follow_the_documented_rules() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(&dir);
    let q = |filter: Filter| Query::new().filter(filter);

    // Nulls fail every test but `IS NULL`.
    assert_eq!(find(&db, q(Filter::ne("handle", "ace"))), [3]);
    assert_eq!(find(&db, q(Filter::is_null("handle"))), [2, 4]);
    assert_eq!(find(&db, q(Filter::eq("handle", Value::Null))), [2, 4]);
    assert_eq!(find(&db, q(!Filter::is_null("handle"))), [1, 3]);

    // A list holds when an element does; an empty list is not null.
    assert_eq!(find(&db, q(Filter::eq("tags", "red"))), [1]);
    assert_eq!(
        find(&db, q(Filter::contains("tags", "re"))),
        Vec::<i64>::new()
    );
    assert_eq!(find(&db, q(Filter::starts_with("tags", "re"))), [1]);
    assert_eq!(find(&db, q(Filter::is_null("tags"))), [3, 4]);

    // `-0.0` equals `0.0`, and NaN equals NaN and sorts after infinity.
    assert_eq!(find(&db, q(Filter::eq("rating", 0.0))), [1, 2]);
    assert_eq!(find(&db, q(Filter::eq("rating", f64::NAN))), [3]);
    assert_eq!(find(&db, q(Filter::gt("rating", f64::INFINITY))), [3]);

    // Strings compare by bytes, zero bytes included.
    assert_eq!(find(&db, q(Filter::starts_with("handle", "a"))), [1, 3]);
    assert_eq!(find(&db, q(Filter::starts_with("handle", "a\0"))), [3]);
    assert_eq!(find(&db, q(Filter::lt("handle", "ab"))), [3]);

    // Links: a missing target reads as null, and a to-many link holds when
    // any target does.
    assert_eq!(find(&db, q(Filter::eq("team.city", "Seoul"))), [1]);
    assert_eq!(find(&db, q(Filter::is_null("team.city"))), [2, 3, 4]);
    assert_eq!(find(&db, q(Filter::eq("friends.score", 3))), [3]);
    assert_eq!(find(&db, q(Filter::eq("address.city", "Busan"))), [2]);

    // Sorting: null first ascending and last descending, ties by key.
    assert_eq!(find(&db, Query::new().sort_by("handle")), [2, 4, 3, 1]);
    assert_eq!(find(&db, Query::new().sort_by_desc("handle")), [1, 3, 2, 4]);
    assert_eq!(find(&db, Query::new().sort_by_desc("score")), [1, 3, 2, 4]);
    assert_eq!(
        find(&db, Query::new().sort_by_desc("score").offset(1).limit(2)),
        [3, 2]
    );

    let read = db.begin_read().unwrap();
    let players = read.collection("players").unwrap();

    assert_eq!(players.count(&q(Filter::eq("score", 3))).unwrap(), 2);
    assert_eq!(players.count(&Query::new().offset(1).limit(2)).unwrap(), 2);
    assert_eq!(players.count(&Query::new().offset(9)).unwrap(), 0);

    for broken in [
        q(Filter::eq("score", "three")),
        q(Filter::eq("score", 3.0)),
        q(Filter::eq("missing", 1)),
        q(Filter::eq("address", 1)),
        q(Filter::gt("handle", Value::Null)),
        q(Filter::starts_with("score", "1")),
        q(Filter::eq("score.value", 1)),
        q(Filter::eq("tags", vec![Value::from("red")])),
        Query::new().sort_by("tags"),
        Query::new().sort_by("friends.score"),
    ] {
        assert_eq!(
            players.query(&broken).err().map(|error| error.code()),
            Some("INVALID_QUERY"),
            "{broken:?}"
        );
    }
}

#[test]
fn a_filter_nested_too_deeply_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(&dir);
    let mut filter = Filter::is_null("handle");

    for _ in 0..ir::MAX_DEPTH {
        filter = !filter;
    }

    let read = db.begin_read().unwrap();

    assert_eq!(
        read.collection("players")
            .unwrap()
            .query(&Query::new().filter(filter))
            .err()
            .map(|error| error.code()),
        Some("INVALID_QUERY")
    );
}
