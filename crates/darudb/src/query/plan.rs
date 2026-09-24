//! Checking a query against the schema, and choosing how to find its objects
//! (`design/objects.md`, "Running a query").
//!
//! Checking resolves every path to the steps that read it and the type of the
//! values at its end, and refuses a value of another type. Choosing looks at
//! the terms of the filter's top-level `AND` for one that the primary key or
//! an index answers, and at the sort for one that the walk can deliver in
//! order. The choice only decides which objects are read and in what order:
//! every object read is still tested against the whole filter unless the
//! access answers the filter exactly.

use std::ops::Bound;

use super::ir::{Expr, Ir, MAX_DEPTH, Op};
use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::key;
use crate::format::object::schema::{CollectionDef, FieldDef, IndexDef, Kind, StoredSchema};

/// How many names a path may have. A path through a link back into its own
/// collection could otherwise go on for as long as the text does, and reading
/// it recurses once a step.
pub(crate) const MAX_PATH: usize = 32;

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidQuery {
        message: message.into(),
    }
}

/// One step of reading a path from an object.
#[derive(Debug)]
pub(crate) enum Step<'s> {
    /// The field of this name of an object, null if the object is null.
    Field(&'s str),
    /// Each element of a list, one null if the list is null.
    Expand,
    /// The object of this collection a link names, null if there is none.
    Follow(&'s CollectionDef),
}

/// A path, resolved against the schema.
#[derive(Debug)]
pub(crate) struct Resolved<'s> {
    pub(crate) steps: Vec<Step<'s>>,
    /// The type of the values at the end: a scalar, a link's being its
    /// target's key type.
    pub(crate) leaf: Kind,
    /// Whether the path ends in a list field, whose elements are the values.
    pub(crate) list_leaf: bool,
    /// Whether the path reads more than one value: it passes a list.
    pub(crate) many: bool,
    /// The field, when the path is a field of the collection itself.
    pub(crate) direct: Option<&'s FieldDef>,
}

/// A test of the values at a path.
#[derive(Debug)]
pub(crate) enum Test {
    /// `==`, `!=`, `<`, `<=`, `>` or `>=` with a value.
    Compare(Op, Value),
    Between(Value, Value),
    /// Equal to one of the values, which are sorted and without repeats.
    In(Vec<Value>),
    /// `CONTAINS` on a list: equal to the value.
    Element(Value),
    /// `CONTAINS` on a string.
    Substring(String),
    StartsWith(String),
    EndsWith(String),
    IsNull,
}

/// A filter, resolved against the schema.
#[derive(Debug)]
pub(crate) enum Cond<'s> {
    And(Vec<Cond<'s>>),
    Or(Vec<Cond<'s>>),
    Not(Box<Cond<'s>>),
    Test { path: Resolved<'s>, test: Test },
}

/// A range of keys.
pub(crate) type Range = (Bound<Vec<u8>>, Bound<Vec<u8>>);

/// How the objects are found.
#[derive(Debug)]
pub(crate) enum Access<'s> {
    /// The collection's records within a range of primary keys.
    Records { range: Range, backward: bool },
    /// The records of these primary keys, sorted, without repeats.
    Keys { keys: Vec<Vec<u8>>, backward: bool },
    /// The entries of an index within these ranges, which are sorted and do
    /// not overlap.
    Index {
        index: &'s IndexDef,
        ranges: Vec<Range>,
        backward: bool,
        /// Whether an object can have more than one entry in the ranges, so
        /// that its key has to be remembered to be read once.
        repeats: bool,
    },
}

/// A query, checked and planned.
#[derive(Debug)]
pub(crate) struct Plan<'s> {
    pub(crate) collection: &'s CollectionDef,
    /// What each object read has to meet, `None` when every object read
    /// does.
    pub(crate) filter: Option<Cond<'s>>,
    pub(crate) sort: Vec<(Resolved<'s>, bool)>,
    pub(crate) offset: u64,
    pub(crate) limit: Option<u64>,
    pub(crate) access: Access<'s>,
    /// Whether the access delivers the objects in the query's order.
    pub(crate) ordered: bool,
    /// Whether the query has a filter that the access answers alone, so that
    /// counting needs only the keys.
    pub(crate) exact: bool,
}

/// Checks `ir` against `collection` of `schema`, and plans it.
pub(crate) fn plan<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    ir: &Ir,
) -> Result<Plan<'s>> {
    let (filter, sort) = resolve(schema, collection, ir)?;
    let (access, consumed, ordered) = choose(collection, filter.as_ref(), &sort);
    let (filter, exact) = match filter {
        Some(Cond::And(terms)) if !consumed.is_empty() => {
            let rest: Vec<Cond<'s>> = terms
                .into_iter()
                .enumerate()
                .filter(|(position, _)| !consumed.contains(position))
                .map(|(_, term)| term)
                .collect();

            if rest.is_empty() {
                (None, true)
            } else {
                (Some(Cond::And(rest)), false)
            }
        }
        Some(cond) if consumed == [0] && !matches!(cond, Cond::And(_)) => (None, true),
        filter => (filter, false),
    };

    Ok(Plan {
        collection,
        filter,
        sort,
        offset: ir.offset,
        limit: ir.limit,
        access,
        ordered,
        exact,
    })
}

/// The plan every query also has: a walk of every record, each tested
/// against the whole filter, sorted in memory. The tests compare every
/// other plan with it.
#[cfg(test)]
pub(crate) fn scan<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    ir: &Ir,
) -> Result<Plan<'s>> {
    let (filter, sort) = resolve(schema, collection, ir)?;

    Ok(Plan {
        collection,
        filter,
        sort,
        offset: ir.offset,
        limit: ir.limit,
        access: Access::Records {
            range: (Bound::Unbounded, Bound::Unbounded),
            backward: false,
        },
        ordered: false,
        exact: false,
    })
}

/// The filter and the sort of `ir`, resolved against the schema.
#[expect(clippy::type_complexity, reason = "a pair of the two parts of a query")]
fn resolve<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    ir: &Ir,
) -> Result<(Option<Cond<'s>>, Vec<(Resolved<'s>, bool)>)> {
    let resolver = Resolver { schema, collection };
    let filter = ir
        .filter
        .as_ref()
        .map(|expr| resolver.cond(expr, 1))
        .transpose()?;
    let sort = ir
        .sort
        .iter()
        .map(|(path, descending)| {
            let resolved = resolver.path(path)?;

            if resolved.many || resolved.list_leaf {
                return Err(invalid(format!(
                    "`{}` holds several values, so it cannot be sorted by",
                    path.join(".")
                )));
            }

            Ok((resolved, *descending))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((filter, sort))
}

struct Resolver<'s> {
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
}

impl<'s> Resolver<'s> {
    fn cond(&self, expr: &Expr, depth: usize) -> Result<Cond<'s>> {
        if depth > MAX_DEPTH {
            return Err(invalid(format!(
                "the filter nests more than {MAX_DEPTH} levels deep"
            )));
        }

        Ok(match expr {
            Expr::And(terms) => Cond::And(
                terms
                    .iter()
                    .map(|term| self.cond(term, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Expr::Or(terms) => Cond::Or(
                terms
                    .iter()
                    .map(|term| self.cond(term, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Expr::Not(term) => Cond::Not(Box::new(self.cond(term, depth + 1)?)),
            Expr::Test { op, path, values } => {
                let resolved = self.path(path)?;
                let test = test(*op, &resolved, values, &path.join("."))?;

                Cond::Test {
                    path: resolved,
                    test,
                }
            }
        })
    }

    fn path(&self, path: &[String]) -> Result<Resolved<'s>> {
        if path.len() > MAX_PATH {
            return Err(invalid(format!(
                "a path has at most {MAX_PATH} names, and one has {}",
                path.len()
            )));
        }

        let text = path.join(".");
        let mut fields = &self.collection.fields;
        let mut steps = Vec::new();
        let mut many = false;
        let mut direct = None;

        for (position, name) in path.iter().enumerate() {
            let last = position + 1 == path.len();
            let field = fields.by_name(name).ok_or_else(|| {
                invalid(format!(
                    "`{text}` names `{name}`, which is not a field there"
                ))
            })?;

            if position == 0 && last {
                direct = Some(field);
            }

            steps.push(Step::Field(&field.name));

            let kind = match &field.kind {
                Kind::List(element) => {
                    steps.push(Step::Expand);
                    many = true;
                    element.as_ref()
                }
                kind => kind,
            };

            match kind {
                Kind::Object(inner) if !last => fields = inner,
                Kind::Object(_) => {
                    return Err(invalid(format!(
                        "`{text}` is an embedded object; a query tests one of its fields"
                    )));
                }
                Kind::Link { collection } => {
                    let target = self.schema.collection_by_id(*collection).ok_or_else(|| {
                        invalid(format!("`{text}` links to a collection the schema lacks"))
                    })?;

                    if last {
                        let leaf =
                            target
                                .key_field()
                                .map(|key| key.kind.clone())
                                .ok_or_else(|| {
                                    invalid(format!("`{text}` links to a keyless collection"))
                                })?;

                        return Ok(Resolved {
                            steps,
                            leaf,
                            list_leaf: matches!(field.kind, Kind::List(_)),
                            many,
                            direct,
                        });
                    }

                    steps.push(Step::Follow(target));
                    fields = &target.fields;
                }
                scalar if last => {
                    return Ok(Resolved {
                        steps,
                        leaf: scalar.clone(),
                        list_leaf: matches!(field.kind, Kind::List(_)),
                        many,
                        direct,
                    });
                }
                _ => {
                    return Err(invalid(format!(
                        "`{text}` goes on past `{name}`, which has no fields"
                    )));
                }
            }
        }

        Err(invalid("a path is empty"))
    }
}

/// The largest integer every smaller one of which a float holds exactly.
const EXACT_FLOAT: i64 = 1 << 53;

/// An int compared with a float field, as the float it equals, when it has
/// one. A language with one type of number, such as JavaScript, cannot tell
/// `1` from `1.0`, so the query that writes either means the float.
fn exact_float(value: &Value, leaf: &Kind) -> Option<Value> {
    match (value, leaf) {
        (Value::Int(int), Kind::Float) if (-EXACT_FLOAT..=EXACT_FLOAT).contains(int) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the range checked above converts exactly"
            )]
            let float = *int as f64;

            Some(Value::Float(float))
        }
        _ => None,
    }
}

/// Whether `value` is a value of the scalar `kind`.
fn fits(value: &Value, kind: &Kind) -> bool {
    matches!(
        (kind, value),
        (Kind::Bool, Value::Bool(_))
            | (Kind::Int, Value::Int(_))
            | (Kind::Float, Value::Float(_))
            | (Kind::String, Value::String(_))
            | (Kind::Bytes, Value::Bytes(_))
    )
}

fn test(op: Op, path: &Resolved<'_>, values: &[Value], text: &str) -> Result<Test> {
    let check = |value: &Value| {
        if fits(value, &path.leaf) {
            Ok(value.clone())
        } else if let Some(float) = exact_float(value, &path.leaf) {
            Ok(float)
        } else if value.is_null() {
            Err(invalid(format!(
                "`{text}` is tested with `{}` against null, which only `==` and `!=` do",
                op.text()
            )))
        } else {
            Err(invalid(format!(
                "`{text}` holds {}, and the query tests it with `{}` against {value:?}",
                path.leaf.describe(),
                op.text()
            )))
        }
    };
    let string = |value: &Value| match (&path.leaf, value) {
        (Kind::String, Value::String(text)) => Ok(text.clone()),
        _ => Err(invalid(format!(
            "`{}` tests strings, and `{text}` holds {} or it is tested against {value:?}",
            op.text(),
            path.leaf.describe()
        ))),
    };

    Ok(match (op, values) {
        (Op::IsNull, []) => Test::IsNull,
        (Op::Between, [low, high]) => Test::Between(check(low)?, check(high)?),
        (Op::In, values) => {
            let mut values = values.iter().map(check).collect::<Result<Vec<_>>>()?;

            values.sort_by(key::compare);
            values.dedup_by(|a, b| key::compare(a, b).is_eq());

            Test::In(values)
        }
        (Op::Contains, [value]) if path.list_leaf => Test::Element(check(value)?),
        (Op::Contains, [value]) => Test::Substring(string(value)?),
        (Op::StartsWith, [value]) => Test::StartsWith(string(value)?),
        (Op::EndsWith, [value]) => Test::EndsWith(string(value)?),
        (Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge, [value]) => {
            Test::Compare(op, check(value)?)
        }
        _ => {
            return Err(invalid(format!(
                "`{}` on `{text}` has the wrong number of values",
                op.text()
            )));
        }
    })
}

/// Where a scalar kind's encodings lie: from its first tag up to, but not
/// including, the next.
fn type_range(kind: &Kind) -> Range {
    let (first, next) = match kind {
        Kind::Bool => (0x02, 0x04),
        Kind::Int => (0x04, 0x05),
        Kind::Float => (0x05, 0x06),
        Kind::String => (0x06, 0x07),
        _ => (0x07, 0x08),
    };

    (Bound::Included(vec![first]), Bound::Excluded(vec![next]))
}

fn encoded(value: &Value) -> Vec<u8> {
    // Every value here was checked to be a scalar.
    key::encoded(value).unwrap_or_default()
}

/// The keys after every key that starts with `prefix`.
fn after(prefix: &[u8]) -> Bound<Vec<u8>> {
    key::after_prefix(prefix).map_or(Bound::Unbounded, Bound::Excluded)
}

/// Where a term can be answered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Key,
    Unique,
    Index,
}

/// What kind of lookup a term makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lookup {
    Equal,
    Several,
    Range,
}

/// A term of the filter that the primary key or an index answers.
struct Candidate<'s> {
    term: usize,
    field: &'s FieldDef,
    /// The type of the field's values, a link's being its target's key type.
    leaf: Kind,
    source: Source,
    index: Option<&'s IndexDef>,
    lookup: Lookup,
    /// The values looked up, for `Equal` and `Several`.
    values: Vec<Value>,
    /// The range of values, for `Range`, as bounds on the values.
    low: Bound<Value>,
    high: Bound<Value>,
    prefix: Option<String>,
}

impl Candidate<'_> {
    /// How good the lookup is: equality before several values before a
    /// range, and within each, the primary key before a unique index before
    /// another.
    fn rank(&self) -> (u8, Source) {
        let lookup = match (self.lookup, self.source) {
            (Lookup::Equal, Source::Key | Source::Unique) => 0,
            (Lookup::Several, Source::Key | Source::Unique) => 1,
            (Lookup::Equal, Source::Index) => 2,
            (Lookup::Several, Source::Index) => 3,
            (Lookup::Range, _) => 4,
        };

        (lookup, self.source)
    }
}

fn candidate<'s>(
    collection: &'s CollectionDef,
    term: usize,
    cond: &Cond<'s>,
) -> Option<Candidate<'s>> {
    let Cond::Test { path, test } = cond else {
        return None;
    };
    let field = path.direct?;
    let (source, index) = if field.id == collection.key {
        (Source::Key, None)
    } else {
        let index = collection
            .indexes
            .iter()
            .find(|index| index.field == field.id)?;

        (
            if index.unique {
                Source::Unique
            } else {
                Source::Index
            },
            Some(index),
        )
    };
    let mut found = Candidate {
        term,
        field,
        leaf: path.leaf.clone(),
        source,
        index,
        lookup: Lookup::Range,
        values: Vec::new(),
        low: Bound::Unbounded,
        high: Bound::Unbounded,
        prefix: None,
    };

    match test {
        Test::Compare(Op::Eq, value) | Test::Element(value) => {
            found.lookup = Lookup::Equal;
            found.values = vec![value.clone()];
        }
        Test::IsNull if source != Source::Key => {
            found.lookup = Lookup::Equal;
            found.values = vec![Value::Null];
        }
        Test::In(values) => {
            found.lookup = Lookup::Several;
            found.values.clone_from(values);
        }
        Test::Compare(Op::Lt, value) => found.high = Bound::Excluded(value.clone()),
        Test::Compare(Op::Le, value) => found.high = Bound::Included(value.clone()),
        Test::Compare(Op::Gt, value) => found.low = Bound::Excluded(value.clone()),
        Test::Compare(Op::Ge, value) => found.low = Bound::Included(value.clone()),
        Test::Between(low, high) => {
            found.low = Bound::Included(low.clone());
            found.high = Bound::Included(high.clone());
        }
        Test::StartsWith(prefix) => found.prefix = Some(prefix.clone()),
        _ => return None,
    }

    Some(found)
}

/// The tighter of two lower bounds on values.
fn higher_low(a: Bound<Value>, b: Bound<Value>) -> Bound<Value> {
    match (&a, &b) {
        (Bound::Unbounded, _) => b,
        (_, Bound::Unbounded) => a,
        (Bound::Included(x) | Bound::Excluded(x), Bound::Included(y) | Bound::Excluded(y)) => {
            match key::compare(x, y) {
                std::cmp::Ordering::Less => b,
                std::cmp::Ordering::Greater => a,
                std::cmp::Ordering::Equal if matches!(a, Bound::Excluded(_)) => a,
                std::cmp::Ordering::Equal => b,
            }
        }
    }
}

/// The tighter of two upper bounds on values.
fn lower_high(a: Bound<Value>, b: Bound<Value>) -> Bound<Value> {
    match (&a, &b) {
        (Bound::Unbounded, _) => b,
        (_, Bound::Unbounded) => a,
        (Bound::Included(x) | Bound::Excluded(x), Bound::Included(y) | Bound::Excluded(y)) => {
            match key::compare(x, y) {
                std::cmp::Ordering::Less => a,
                std::cmp::Ordering::Greater => b,
                std::cmp::Ordering::Equal if matches!(a, Bound::Excluded(_)) => a,
                std::cmp::Ordering::Equal => b,
            }
        }
    }
}

/// The key range of the values from `low` to `high` of a field of `kind`, and
/// starting with `prefix` if there is one. An index entry that starts with a
/// value's encoding sorts with that value, so a bound that excludes a value
/// is placed after every entry of it.
fn range(
    kind: &Kind,
    low: &Bound<Value>,
    high: &Bound<Value>,
    prefix: Option<&str>,
    entries: bool,
) -> Range {
    let (mut start, mut end) = type_range(kind);

    match low {
        Bound::Included(value) => start = Bound::Included(encoded(value)),
        // An encoding starts with its tag, which is never `0xFF`, so there
        // is always a key after every entry of a value.
        Bound::Excluded(value) if entries => {
            if let Bound::Excluded(bytes) = after(&encoded(value)) {
                start = Bound::Included(bytes);
            }
        }
        Bound::Excluded(value) => start = Bound::Excluded(encoded(value)),
        Bound::Unbounded => {}
    }

    match high {
        Bound::Included(value) if entries => end = after(&encoded(value)),
        Bound::Included(value) => end = Bound::Included(encoded(value)),
        Bound::Excluded(value) => end = Bound::Excluded(encoded(value)),
        Bound::Unbounded => {}
    }

    if let Some(prefix) = prefix {
        let bytes = key::string_prefix(prefix);

        start = tighter_start(start, Bound::Included(bytes.clone()));
        end = tighter_end(end, after(&bytes));
    }

    (start, end)
}

fn tighter_start(a: Bound<Vec<u8>>, b: Bound<Vec<u8>>) -> Bound<Vec<u8>> {
    match (&a, &b) {
        (Bound::Unbounded, _) => b,
        (_, Bound::Unbounded) => a,
        (Bound::Included(x) | Bound::Excluded(x), Bound::Included(y) | Bound::Excluded(y)) => {
            if x > y || (x == y && matches!(a, Bound::Excluded(_))) {
                a
            } else {
                b
            }
        }
    }
}

fn tighter_end(a: Bound<Vec<u8>>, b: Bound<Vec<u8>>) -> Bound<Vec<u8>> {
    match (&a, &b) {
        (Bound::Unbounded, _) => b,
        (_, Bound::Unbounded) => a,
        (Bound::Included(x) | Bound::Excluded(x), Bound::Included(y) | Bound::Excluded(y)) => {
            if x < y || (x == y && matches!(a, Bound::Excluded(_))) {
                a
            } else {
                b
            }
        }
    }
}

/// The access for a query, the positions of the top-level terms it answers,
/// and whether it delivers the query's order.
fn choose<'s>(
    collection: &'s CollectionDef,
    filter: Option<&Cond<'s>>,
    sort: &[(Resolved<'s>, bool)],
) -> (Access<'s>, Vec<usize>, bool) {
    let terms: Vec<&Cond<'s>> = match filter {
        Some(Cond::And(terms)) => terms.iter().collect(),
        Some(cond) => vec![cond],
        None => Vec::new(),
    };
    let candidates: Vec<Candidate<'s>> = terms
        .iter()
        .enumerate()
        .filter_map(|(position, cond)| candidate(collection, position, cond))
        .collect();
    // The field the query sorts by alone, and whether descending.
    let sorted_by = match sort {
        [(path, descending)] => path.direct.map(|field| (field.id, *descending)),
        _ => None,
    };
    let best = candidates
        .iter()
        .min_by_key(|candidate| (candidate.rank(), candidate.term));

    let Some(best) = best else {
        return unfiltered(collection, sort, sorted_by);
    };
    let backward =
        sorted_by.is_some_and(|(field, descending)| field == best.field.id && descending);
    let follows_sort = sorted_by.is_some_and(|(field, _)| field == best.field.id);
    let list = matches!(best.field.kind, Kind::List(_));

    match (best.lookup, best.source) {
        (Lookup::Equal | Lookup::Several, Source::Key) => {
            let keys: Vec<Vec<u8>> = best.values.iter().map(encoded).collect();

            (
                Access::Keys { keys, backward },
                vec![best.term],
                sort.is_empty() || follows_sort,
            )
        }
        (Lookup::Equal | Lookup::Several, _) => {
            let ranges: Vec<Range> = best
                .values
                .iter()
                .map(|value| {
                    let bytes = encoded(value);
                    let end = after(&bytes);

                    (Bound::Included(bytes), end)
                })
                .collect();
            let single = ranges.len() == 1;

            (
                Access::Index {
                    index: best
                        .index
                        .unwrap_or_else(|| unreachable!("an index candidate has its index")),
                    ranges,
                    backward,
                    repeats: list && !single,
                },
                vec![best.term],
                (sort.is_empty() && single) || (follows_sort && !list),
            )
        }
        (Lookup::Range, source) => {
            // Every range term on the same field narrows the one range. Not
            // on a list: each term holds when any element meets it, and two
            // elements can meet two terms that no one value meets together.
            let mut low = Bound::Unbounded;
            let mut high = Bound::Unbounded;
            let mut prefix: Option<String> = None;
            let mut consumed = Vec::new();

            for candidate in &candidates {
                let merges =
                    candidate.term == best.term || (!list && candidate.field.id == best.field.id);

                if candidate.lookup != Lookup::Range || !merges {
                    continue;
                }

                if candidate.prefix.is_some() && prefix.is_some() {
                    continue;
                }

                low = higher_low(low, candidate.low.clone());
                high = lower_high(high, candidate.high.clone());
                prefix = prefix.or_else(|| candidate.prefix.clone());
                consumed.push(candidate.term);
            }

            let entries = source != Source::Key;
            let range = range(&best.leaf, &low, &high, prefix.as_deref(), entries);

            match best.index {
                None => (
                    Access::Records { range, backward },
                    consumed,
                    sort.is_empty() || follows_sort,
                ),
                Some(index) => (
                    Access::Index {
                        index,
                        ranges: vec![range],
                        backward,
                        repeats: list,
                    },
                    consumed,
                    follows_sort && !list,
                ),
            }
        }
    }
}

/// The access for a query whose filter no key or index answers: the index
/// of the field it sorts by, if it sorts by one alone, or every record.
fn unfiltered<'s>(
    collection: &'s CollectionDef,
    sort: &[(Resolved<'s>, bool)],
    sorted_by: Option<(u64, bool)>,
) -> (Access<'s>, Vec<usize>, bool) {
    let all = (Bound::Unbounded, Bound::Unbounded);

    match sorted_by {
        Some((field, descending)) if field == collection.key => (
            Access::Records {
                range: all,
                backward: descending,
            },
            Vec::new(),
            true,
        ),
        Some((field, descending)) => {
            match collection.indexes.iter().find(|index| index.field == field) {
                Some(index) => (
                    Access::Index {
                        index,
                        ranges: vec![all],
                        backward: descending,
                        repeats: false,
                    },
                    Vec::new(),
                    true,
                ),
                None => (
                    Access::Records {
                        range: all,
                        backward: false,
                    },
                    Vec::new(),
                    false,
                ),
            }
        }
        None => (
            Access::Records {
                range: all,
                backward: false,
            },
            Vec::new(),
            sort.is_empty(),
        ),
    }
}
