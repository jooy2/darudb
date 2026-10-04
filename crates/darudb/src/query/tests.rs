//! Every query gives the scan's answer: random queries over random data, run
//! through the planner and as a walk of every record, return the same
//! objects in the same order, and count the same.

use super::build::{Filter, Query};
use super::ir::{self, Ir};
use super::{plan, run};
use crate::format::object::codec::NameOrder;
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

thread_local! {
    /// How many queries [`agree`] found to be lookups of one value.
    static POINTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Runs `ir`, with `parameters` for its parameters, through the planner and
/// as a scan, and checks the two agree. Returns the planned objects' ids, or
/// `None` for an invalid query.
fn agree(
    source: &dyn Source,
    db_schema: &crate::format::object::schema::StoredSchema,
    ir: &Ir,
    parameters: &[Value],
) -> Option<Vec<i64>> {
    let collection = db_schema.collection("players").unwrap();
    let planned = plan::plan(db_schema, collection, ir, parameters);
    let scanned = plan::scan(db_schema, collection, ir, parameters);

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
    // The planned query's objects are put in order by the collection's order
    // of fields by name, as a handle's schema gives it, and the scan's
    // without one.
    let order = NameOrder::of(&collection.fields);
    let found = ids(&run::objects(source, &planned, Some(&order)).unwrap());
    let expected = ids(&run::whole(|| run::objects(source, &scanned, None)).unwrap());
    let mut lent = Vec::new();

    run::each_stored(source, &planned, &mut |record| {
        lent.push(record.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(lent, run::stored(source, &planned).unwrap(), "{ir:?}");

    // A lookup of one value, looked up without a plan, finds what the plan
    // finds.
    let mut looked_up = Vec::new();

    if run::point(source, collection, ir, parameters, &mut |record| {
        looked_up.push(record.to_vec());
        Ok(())
    })
    .unwrap()
    {
        assert_eq!(looked_up, lent, "{ir:?} looked up");
        POINTS.with(|points| points.set(points.get() + 1));
    }

    assert_eq!(found, expected, "{ir:?}\nplanned {planned:?}");

    // The same query without its offset and limit, cut here: what a sort
    // that keeps only the objects still able to be in the result has to
    // give, found without cutting anything, so that a mistake shared by the
    // plan and the scan shows.
    let everything = Ir {
        offset: 0,
        limit: None,
        ..ir.clone()
    };
    let everything = plan::scan(db_schema, collection, &everything, parameters).unwrap();
    let cut: Vec<i64> = ids(&run::whole(|| run::objects(source, &everything, None)).unwrap())
        .into_iter()
        .skip(usize::try_from(ir.offset).unwrap())
        .take(
            ir.limit
                .map_or(usize::MAX, |limit| usize::try_from(limit).unwrap()),
        )
        .collect();

    assert_eq!(found, cut, "{ir:?}, cut from every object it finds");
    assert_eq!(
        run::count(source, &planned).unwrap(),
        run::whole(|| run::count(source, &scanned)).unwrap(),
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
    // The last seeds hold enough players that one value of an index has more
    // objects than a backward walk holds back (`run::GROUP`).
    for seed in 0..8 {
        let players = if seed < 6 { 30 } else { 400 };
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

        for _ in 0..players {
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
            let bytes = ir::encode("players", query.ir(), false).unwrap();
            let (_, decoded, _) = ir::decode(&bytes).unwrap();

            assert_eq!(ir::encode("players", &decoded, false).unwrap(), bytes);

            let found = agree(&read, &open.schema, query.ir(), &[]);

            if let Some(found) = &found {
                valid += 1;
                nonempty += usize::from(!found.is_empty());
            }

            // The same query prepared with every value a parameter, which
            // the planner reads from the parameters where the IR names them.
            let (prepared, parameters) = prepared(&query);
            let bound = prepared.bind(&parameters).unwrap();

            assert_eq!(
                agree(&read, &open.schema, bound.ir(), &parameters),
                found,
                "{:?} with {parameters:?}",
                bound.ir()
            );
        }

        assert!(valid > 1000, "seed {seed}: only {valid} queries were valid");

        // Lookups of one value, which the random queries seldom are: of the
        // primary key and of a unique field, present and missing, of another
        // type than the field's and of null, built and prepared.
        for _ in 0..100 {
            let value: Value = match rng.below(6) {
                0 | 1 => pick(&mut rng, &HANDLES).into(),
                2 => "nobody".into(),
                3 => (1 + int(&mut rng, players + 5)).into(),
                4 => Value::Null,
                _ => 2.5.into(),
            };
            let field = if rng.below(2) == 0 { "handle" } else { "id" };
            let mut query = Query::new().filter(Filter::eq(field, value));

            if rng.below(3) == 0 {
                query = query.first();
            }

            let found = agree(&read, &open.schema, query.ir(), &[]);
            let (prepared, parameters) = prepared(&query);

            assert_eq!(
                agree(
                    &read,
                    &open.schema,
                    prepared.bind(&parameters).unwrap().ir(),
                    &parameters
                ),
                found
            );
        }

        assert!(
            POINTS.with(std::cell::Cell::get) > 30,
            "seed {seed}: few lookups of one value: {}",
            POINTS.with(std::cell::Cell::get)
        );
        POINTS.with(|points| points.set(0));
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
            let (prepared, parameters) = prepared(&query);

            assert_eq!(
                agree(&txn, &open.schema, prepared.ir(), &parameters),
                agree(&txn, &open.schema, query.ir(), &[])
            );
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

    // Two range terms on a list can hold for two different elements.
    let both = Filter::ge("tags", "red").and(Filter::le("tags", "blue"));

    assert_eq!(find(&db, q(both.clone())), [1]);
    assert_eq!(
        db.begin_read()
            .unwrap()
            .collection("players")
            .unwrap()
            .count(&q(both))
            .unwrap(),
        1
    );
    assert_eq!(find(&db, q(Filter::is_null("tags"))), [3, 4]);

    // `-0.0` equals `0.0`, and NaN equals NaN and sorts after infinity.
    assert_eq!(find(&db, q(Filter::eq("rating", 0.0))), [1, 2]);
    assert_eq!(find(&db, q(Filter::eq("rating", f64::NAN))), [3]);
    assert_eq!(find(&db, q(Filter::gt("rating", f64::INFINITY))), [3]);

    // An int compared with a float field means the float it equals.
    assert_eq!(find(&db, q(Filter::eq("rating", 0))), [1, 2]);
    assert_eq!(
        find(
            &db,
            Query::parse("rating < 1 AND rating >= 0", &[]).unwrap()
        ),
        [1, 2]
    );

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
    assert_eq!(players.count(&Query::new().first()).unwrap(), 1);
    assert_eq!(players.count(&Query::new().limit(0).first()).unwrap(), 0);
    assert_eq!(
        ids(&players
            .query(&Query::new().sort_by_desc("score").first())
            .unwrap()),
        [1]
    );

    for broken in [
        q(Filter::eq("score", "three")),
        q(Filter::eq("score", 3.5)),
        q(Filter::eq("rating", 1i64 << 60)),
        q(Filter::eq("missing", 1)),
        q(Filter::eq("address", 1)),
        q(Filter::gt("handle", Value::Null)),
        q(Filter::starts_with("score", "1")),
        q(Filter::eq("score.value", 1)),
        q(Filter::eq("tags", vec![Value::from("red")])),
        Query::new().sort_by("tags"),
        Query::new().sort_by("friends.score"),
        // Long enough to recurse without end, were paths not bounded.
        q(Filter::is_null(
            &vec!["friends"; plan::MAX_PATH + 1].join("."),
        )),
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

/// `query` prepared from its text with every value a parameter, and the
/// parameters' values.
fn prepared(query: &Query) -> (Query, Vec<Value>) {
    let mut parameters = Vec::new();
    let written = text(query.ir(), &mut parameters, true);
    let prepared = Query::prepare(&written).unwrap_or_else(|error| panic!("{written}: {error}"));

    (prepared, parameters)
}

/// `ir` in the query language. Floats and bytes, which have no literal that
/// keeps every value, go into `parameters`; other values are written out,
/// unless `every` is set, which makes every value a parameter and a null
/// test a comparison with a null parameter.
fn text(ir: &Ir, parameters: &mut Vec<Value>, every: bool) -> String {
    fn name(name: &str) -> String {
        let plain = name
            .chars()
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !super::parse::is_keyword(name);

        if plain {
            name.to_owned()
        } else {
            format!("`{name}`")
        }
    }

    fn path(path: &[String]) -> String {
        path.iter()
            .map(|part| name(part))
            .collect::<Vec<_>>()
            .join(".")
    }

    fn value(value: &Value, parameters: &mut Vec<Value>, every: bool) -> String {
        match value {
            _ if every => {
                parameters.push(value.clone());
                format!("${}", parameters.len() - 1)
            }
            Value::Null => "null".to_owned(),
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::String(text) => {
                let mut out = String::from("\"");

                for c in text.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
                        c => out.push(c),
                    }
                }

                out.push('"');
                out
            }
            other => {
                parameters.push(other.clone());
                format!("${}", parameters.len() - 1)
            }
        }
    }

    fn expr(node: &ir::Expr, parameters: &mut Vec<Value>, every: bool) -> String {
        match node {
            ir::Expr::And(terms) | ir::Expr::Or(terms) => {
                let joint = if matches!(node, ir::Expr::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                let terms: Vec<String> = terms
                    .iter()
                    .map(|term| expr(term, parameters, every))
                    .collect();

                format!("({})", terms.join(joint))
            }
            ir::Expr::Not(term) => match term.as_ref() {
                ir::Expr::Test {
                    op: ir::Op::IsNull,
                    path: at,
                    ..
                } if every => format!("{} != {}", path(at), value(&Value::Null, parameters, true)),
                term => format!("NOT ({})", expr(term, parameters, every)),
            },
            ir::Expr::Prepared { .. } => unreachable!("a builder's query has no parameters"),
            ir::Expr::Test {
                op,
                path: at,
                values,
            } => {
                let at = path(at);

                match (op, values.as_slice()) {
                    (ir::Op::IsNull, _) if every => {
                        format!("{at} == {}", value(&Value::Null, parameters, true))
                    }
                    (ir::Op::IsNull, _) => format!("{at} is null"),
                    (ir::Op::Between, [low, high]) => {
                        let (low, high) = (
                            value(low, parameters, every),
                            value(high, parameters, every),
                        );

                        format!("{at} BETWEEN {low} AND {high}")
                    }
                    (ir::Op::In, values) => {
                        let values: Vec<String> = values
                            .iter()
                            .map(|each| value(each, parameters, every))
                            .collect();

                        format!("{at} in [{}]", values.join(", "))
                    }
                    (op, [one]) => format!("{at} {} {}", op.text(), value(one, parameters, every)),
                    _ => unreachable!("a builder's test has its values"),
                }
            }
        }
    }

    let mut out = Vec::new();

    if let Some(filter) = &ir.filter {
        out.push(expr(filter, parameters, every));
    }

    if !ir.sort.is_empty() {
        let keys: Vec<String> = ir
            .sort
            .iter()
            .map(|(at, descending)| {
                format!("{} {}", path(at), if *descending { "desc" } else { "ASC" })
            })
            .collect();

        out.push(format!("SORT BY {}", keys.join(", ")));
    }

    if let Some(limit) = ir.limit {
        out.push(format!("Limit {limit}"));
    }

    if ir.offset > 0 {
        out.push(format!("OFFSET {}", ir.offset));
    }

    out.join(" ")
}

#[test]
fn the_query_language_builds_the_builders_query() {
    let mut rng = Rng::new(7);

    for _ in 0..3000 {
        let built = query(&mut rng);
        let mut parameters = Vec::new();
        let written = text(built.ir(), &mut parameters, false);
        let parsed = Query::parse(&written, &parameters)
            .unwrap_or_else(|error| panic!("{written}: {error}"));

        // Compared as IR, where NaN equals itself.
        assert_eq!(
            ir::encode("players", parsed.ir(), false).unwrap(),
            ir::encode("players", built.ir(), false).unwrap(),
            "{written}"
        );
    }

    let same = [
        (
            "score >= 1 AND handle STARTSWITH \"a\" OR NOT tags CONTAINS \"red\"",
            Query::new().filter(
                Filter::ge("score", 1)
                    .and(Filter::starts_with("handle", "a"))
                    .or(!Filter::contains("tags", "red")),
            ),
        ),
        (
            "score == null and (`handle` != NULL) SORT BY team.city, score DESC LIMIT 3",
            Query::new()
                .filter(Filter::is_null("score").and(Filter::is_not_null("handle")))
                .sort_by("team.city")
                .sort_by_desc("score")
                .limit(3),
        ),
        (
            "rating between -1.5 and 2e3 AND address.zip IN [] AND note == $0",
            Query::new().filter(
                Filter::between("rating", -1.5, 2000.0)
                    .and(Filter::is_in("address.zip", Vec::<Value>::new()))
                    .and(Filter::eq("note", vec![1u8, 2])),
            ),
        ),
        ("OFFSET 4", Query::new().offset(4)),
        ("", Query::new()),
        (
            "`limit`.`and` == \"\\u{0}\\\"\\\\\\n\\t\" AND a.limit IS NOT NULL",
            Query::new()
                .filter(Filter::eq("limit.and", "\0\"\\\n\t").and(Filter::is_not_null("a.limit"))),
        ),
    ];

    for (written, built) in same {
        assert_eq!(
            Query::parse(written, &[Value::Bytes(vec![1, 2])]).unwrap(),
            built,
            "{written}"
        );
    }
}

#[test]
fn text_that_does_not_parse_names_where() {
    let broken = [
        ("score >", "at character 8: expected a value, found the end"),
        ("score = 1", "at character 7: `=` has no meaning here"),
        ("score == 1 LIMIT", "at character 17: expected a number"),
        ("(score == 1", "at character 12: expected `)`"),
        ("score IN [1 2]", "at character 13: expected `,`"),
        ("score BETWEEN 1 OR 2", "at character 17: expected `AND`"),
        // A field called `limit` needs backticks: the word starts a clause.
        (
            "limit == 1",
            "at character 7: expected a number, found `==`",
        ),
        (
            "score == 1 AND limit == 1",
            "at character 16: expected a field name",
        ),
        (
            "score == $2",
            "at character 10: `$2` names a parameter, and 1 were given",
        ),
        ("name == \"open", "at character 9: a string does not end"),
        ("name == \"\\x\"", "at character 10: a string escapes only"),
        (
            "score == 99999999999999999999",
            "at character 10: `99999999999999999999` does not fit",
        ),
        ("score == 1 SORT score", "at character 17: expected `BY`"),
        (
            "score == 1 LIMIT -1",
            "at character 18: a limit or an offset is not negative",
        ),
        ("score IS 1", "at character 10: expected `NULL`"),
        ("score LIKE 1", "at character 7: expected a comparison"),
        (
            "score == 1 score == 2",
            "at character 12: expected `AND`, `OR`",
        ),
        ("`` == 1", "at character 1: a name in backticks is empty"),
    ];

    for (written, message) in broken {
        let error = Query::parse(written, &[Value::Int(1)]).unwrap_err();

        assert_eq!(error.code(), "INVALID_QUERY", "{written}");
        assert!(
            error.to_string().contains(message),
            "{written}: {error} does not say {message}"
        );
    }

    let deep = format!("{}score == 1", "NOT ".repeat(200));

    assert_eq!(
        Query::parse(&deep, &[]).err().map(|error| error.code()),
        Some("INVALID_QUERY")
    );
}

#[test]
fn a_link_holding_a_key_of_another_type_is_damage() {
    use crate::format::object::codec::{self, Raw};
    use crate::format::object::key;
    use crate::format::object::names::records;

    let dir = tempfile::tempdir().unwrap();
    let db = database(&dir);
    let mut txn = db.begin_write().unwrap();
    let open = txn.schema().cloned().unwrap();
    let players = open.schema.collection("players").unwrap();
    let team = players.fields.by_name("team").unwrap().id;
    // A player whose team, a link to a collection keyed by strings, holds
    // an int: something no write lets through.
    let record = codec::write(&[
        (players.key, Raw::Int(9)),
        (team, Raw::Link(Box::new(Raw::Int(5)))),
    ]);

    txn.insert_in(
        &records(players.id),
        &key::encoded(&Value::Int(9)).unwrap(),
        &record,
    )
    .unwrap();

    assert_eq!(
        txn.collection("players")
            .unwrap()
            .query(&Query::new().filter(Filter::eq("team.city", "Seoul")))
            .err()
            .map(|error| error.code()),
        Some("CORRUPTED")
    );
}

/// Objects written before a field was added do not hold it, and read its
/// default in a filter and a sort as they do when read whole.
#[test]
fn a_field_added_later_reads_as_its_default_in_a_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("added.darudb");
    let things = |version: u64, size: Option<i64>| {
        let collection = Collection::new("things").field("name", Type::String);
        let collection = match size {
            Some(size) => collection.with_default("size", Type::Int, size),
            None => collection,
        };

        Schema::new(version).collection(collection)
    };

    {
        let db = OpenOptions::new()
            .schema(things(1, None))
            .open(&path)
            .unwrap();
        let mut txn = db.begin_write().unwrap();

        for name in ["a", "b"] {
            txn.collection("things")
                .unwrap()
                .insert(Object::new().with("name", name))
                .unwrap();
        }

        txn.commit().unwrap();
    }

    let db = OpenOptions::new()
        .schema(things(2, Some(7)))
        .open(&path)
        .unwrap();
    let mut txn = db.begin_write().unwrap();

    txn.collection("things")
        .unwrap()
        .insert(Object::new().with("name", "c").with("size", 3))
        .unwrap();
    txn.commit().unwrap();

    let read = db.begin_read().unwrap();
    let things = read.collection("things").unwrap();
    let names = |query: Query| -> Vec<String> {
        things
            .query(&query)
            .unwrap()
            .iter()
            .map(|object| object.get("name").unwrap().as_str().unwrap().to_owned())
            .collect()
    };

    assert_eq!(
        names(Query::new().filter(Filter::eq("size", 7))),
        ["a", "b"]
    );
    assert_eq!(names(Query::new().sort_by_desc("size")), ["a", "b", "c"]);
    assert_eq!(
        things
            .count(&Query::new().filter(Filter::gt("size", 5)))
            .unwrap(),
        2
    );
}

/// A prepared query given values finds what the same text parsed with them
/// does, and keeps its parameters through the IR a binding sends.
#[test]
fn a_prepared_query_runs_as_the_parsed_one_does() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(&dir);
    let texts = [
        ("score == $0", vec![Value::Int(3)]),
        (
            "score >= $0 AND handle STARTSWITH $1",
            vec![Value::Int(1), "a".into()],
        ),
        ("handle == $0", vec![Value::Null]),
        ("handle != $0", vec![Value::Null]),
        (
            "score IN [$0, 1, $1] SORT BY score DESC",
            vec![Value::Int(3), Value::Int(7)],
        ),
        (
            "rating BETWEEN $1 AND $0",
            vec![Value::Float(1.0), Value::Float(-1.0)],
        ),
        (
            "tags CONTAINS $0 OR NOT (team == $1)",
            vec!["red".into(), "north".into()],
        ),
    ];

    for (text, parameters) in texts {
        let prepared = Query::prepare(text).unwrap();
        let bound = prepared.bind(&parameters).unwrap();
        let parsed = Query::parse(text, &parameters).unwrap();

        assert_eq!(bound, parsed, "{text}");
        assert_eq!(find(&db, bound), find(&db, parsed), "{text}");

        // Through the IR, as a binding sends a prepared query.
        let bytes = ir::encode("players", prepared.ir(), false).unwrap();
        let (_, decoded, _) = ir::decode(&bytes).unwrap();

        assert_eq!(&decoded, prepared.ir(), "{text}");
        assert_eq!(
            Query::from_ir(decoded).bind(&parameters).unwrap(),
            Query::parse(text, &parameters).unwrap()
        );
    }

    let prepared = Query::prepare("score == $0 AND handle == $2").unwrap();

    assert_eq!(
        prepared.bind(&[Value::Int(1)]).unwrap_err().code(),
        "INVALID_QUERY",
        "a parameter without a value"
    );

    let read = db.begin_read().unwrap();
    let error = read
        .collection("players")
        .unwrap()
        .query(&prepared)
        .unwrap_err();

    assert_eq!(error.code(), "INVALID_QUERY");
    assert!(error.to_string().contains("$0"), "{error}");
    assert_eq!(
        Query::prepare("score == 3").unwrap(),
        Query::parse("score == 3", &[]).unwrap(),
        "a query without parameters prepares as it parses"
    );
}
