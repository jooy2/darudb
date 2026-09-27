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

use std::borrow::Cow;
use std::ops::{Bound, Deref};

use super::ir::{Expr, Ir, MAX_DEPTH, Op, Operand};
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
#[derive(Debug, Clone, Copy)]
pub(crate) enum Step<'s> {
    /// The field of this name of an object, null if the object is null.
    Field(&'s str),
    /// Each element of a list, one null if the list is null.
    Expand,
    /// The object of this collection a link names, null if there is none.
    Follow(&'s CollectionDef),
}

/// How many steps a path keeps without a vector of its own: a field, a
/// field of a list, or a field through a link.
const INLINE_STEPS: usize = 3;

/// The steps of a path. Nearly every path is short, and a query plans one
/// for each test and each sort key, so a short one is kept inline rather
/// than in a vector of its own.
#[derive(Debug)]
pub(crate) enum Steps<'s> {
    Inline(usize, [Step<'s>; INLINE_STEPS]),
    Heap(Vec<Step<'s>>),
}

impl<'s> Steps<'s> {
    fn new() -> Self {
        Steps::Inline(0, [Step::Expand; INLINE_STEPS])
    }

    fn push(&mut self, step: Step<'s>) {
        match self {
            Steps::Inline(len, steps) if *len < INLINE_STEPS => {
                steps[*len] = step;
                *len += 1;
            }
            Steps::Inline(_, steps) => {
                let mut heap = steps.to_vec();

                heap.push(step);
                *self = Steps::Heap(heap);
            }
            Steps::Heap(steps) => steps.push(step),
        }
    }
}

impl<'s> Deref for Steps<'s> {
    type Target = [Step<'s>];

    fn deref(&self) -> &[Step<'s>] {
        match self {
            Steps::Inline(len, steps) => &steps[..*len],
            Steps::Heap(steps) => steps,
        }
    }
}

/// A path, resolved against the schema.
#[derive(Debug)]
pub(crate) struct Resolved<'s> {
    pub(crate) steps: Steps<'s>,
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

/// A test of the values at a path. Its values are borrowed from the query
/// or its parameters, except a number made the float it equals.
#[derive(Debug)]
pub(crate) enum Test<'s> {
    /// `==`, `!=`, `<`, `<=`, `>` or `>=` with a value.
    Compare(Op, Cow<'s, Value>),
    Between(Cow<'s, Value>, Cow<'s, Value>),
    /// Equal to one of the values, which are sorted and without repeats.
    In(Vec<Cow<'s, Value>>),
    /// `CONTAINS` on a list: equal to the value.
    Element(Cow<'s, Value>),
    /// `CONTAINS` on a string.
    Substring(&'s str),
    StartsWith(&'s str),
    EndsWith(&'s str),
    IsNull,
}

/// A filter, resolved against the schema.
#[derive(Debug)]
pub(crate) enum Cond<'s> {
    And(Vec<Cond<'s>>),
    Or(Vec<Cond<'s>>),
    Not(Box<Cond<'s>>),
    Test { path: Resolved<'s>, test: Test<'s> },
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
        /// Whether each range holds the entries of one value, as an equality
        /// or an `IN` makes it.
        values: bool,
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

/// Checks `ir` against `collection` of `schema`, with `parameters` for its
/// parameters, and plans it.
pub(crate) fn plan<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    ir: &'s Ir,
    parameters: &'s [Value],
) -> Result<Plan<'s>> {
    let (filter, sort) = resolve(schema, collection, ir, parameters)?;
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
    ir: &'s Ir,
    parameters: &'s [Value],
) -> Result<Plan<'s>> {
    let (filter, sort) = resolve(schema, collection, ir, parameters)?;

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
    ir: &'s Ir,
    parameters: &'s [Value],
) -> Result<(Option<Cond<'s>>, Vec<(Resolved<'s>, bool)>)> {
    let resolver = Resolver {
        schema,
        collection,
        parameters,
    };
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
    /// The values of a bound prepared query's parameters.
    parameters: &'s [Value],
}

impl<'s> Resolver<'s> {
    fn cond(&self, expr: &'s Expr, depth: usize) -> Result<Cond<'s>> {
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
            Expr::Prepared { .. } if self.parameters.is_empty() => {
                return Err(invalid(format!(
                    "`${}` has no value: a prepared query runs with its parameters",
                    expr.first_param().unwrap_or_default()
                )));
            }
            Expr::Prepared { op, path, values } => {
                let operands = Operands::Prepared(values, self.parameters);

                // What `Expr::test` makes of a test with null, when a
                // parameter's value is the null.
                match (op, operands.single()?) {
                    (Op::Eq, Some(Value::Null)) => self.test(Op::IsNull, path, Operands::None)?,
                    (Op::Ne, Some(Value::Null)) => {
                        Cond::Not(Box::new(self.test(Op::IsNull, path, Operands::None)?))
                    }
                    _ => self.test(*op, path, operands)?,
                }
            }
            Expr::Test { op, path, values } => self.test(*op, path, Operands::Values(values))?,
        })
    }

    fn test(&self, op: Op, path: &'s [String], operands: Operands<'s>) -> Result<Cond<'s>> {
        let resolved = self.path(path)?;
        let test = test(op, &resolved, operands, path)?;

        Ok(Cond::Test {
            path: resolved,
            test,
        })
    }

    fn path(&self, path: &[String]) -> Result<Resolved<'s>> {
        if path.len() > MAX_PATH {
            return Err(invalid(format!(
                "a path has at most {MAX_PATH} names, and one has {}",
                path.len()
            )));
        }

        // Joined only for an error.
        let text = || path.join(".");
        let mut fields = &self.collection.fields;
        let mut steps = Steps::new();
        let mut many = false;
        let mut direct = None;

        for (position, name) in path.iter().enumerate() {
            let last = position + 1 == path.len();
            let field = fields.by_name(name).ok_or_else(|| {
                invalid(format!(
                    "`{}` names `{name}`, which is not a field there",
                    text()
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
                        "`{}` is an embedded object; a query tests one of its fields",
                        text()
                    )));
                }
                Kind::Link { collection } => {
                    let target = self.schema.collection_by_id(*collection).ok_or_else(|| {
                        invalid(format!(
                            "`{}` links to a collection the schema lacks",
                            text()
                        ))
                    })?;

                    if last {
                        let leaf =
                            target
                                .key_field()
                                .map(|key| key.kind.clone())
                                .ok_or_else(|| {
                                    invalid(format!("`{}` links to a keyless collection", text()))
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
                        "`{}` goes on past `{name}`, which has no fields",
                        text()
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

/// The values of a test: its own, those of a prepared test with its
/// parameters' values, or none.
#[derive(Clone, Copy)]
enum Operands<'s> {
    Values(&'s [Value]),
    Prepared(&'s [Operand], &'s [Value]),
    None,
}

impl<'s> Operands<'s> {
    fn len(self) -> usize {
        match self {
            Operands::Values(values) => values.len(),
            Operands::Prepared(operands, _) => operands.len(),
            Operands::None => 0,
        }
    }

    fn get(self, at: usize) -> Result<&'s Value> {
        match self {
            Operands::Values(values) => Ok(&values[at]),
            Operands::Prepared(operands, parameters) => match &operands[at] {
                Operand::Value(value) => Ok(value),
                Operand::Param(index) => parameters
                    .get(*index)
                    .ok_or_else(|| super::ir::missing(*index, parameters.len())),
            },
            Operands::None => Err(invalid("a test has no value")),
        }
    }

    /// The one value, if there is exactly one.
    fn single(self) -> Result<Option<&'s Value>> {
        match self.len() {
            1 => self.get(0).map(Some),
            _ => Ok(None),
        }
    }
}

/// The test of `operands` with `op` at `path`, whose names are `names`:
/// those are joined only for an error, since a query that plans joins none.
fn test<'s>(
    op: Op,
    path: &Resolved<'_>,
    operands: Operands<'s>,
    names: &[String],
) -> Result<Test<'s>> {
    let text = || names.join(".");
    let check = |value: &'s Value| {
        if fits(value, &path.leaf) {
            Ok(Cow::Borrowed(value))
        } else if let Some(float) = exact_float(value, &path.leaf) {
            Ok(Cow::Owned(float))
        } else if value.is_null() {
            Err(invalid(format!(
                "`{}` is tested with `{}` against null, which only `==` and `!=` do",
                text(),
                op.text()
            )))
        } else {
            Err(invalid(format!(
                "`{}` holds {}, and the query tests it with `{}` against {value:?}",
                text(),
                path.leaf.describe(),
                op.text()
            )))
        }
    };
    let string = |value: &'s Value| match (&path.leaf, value) {
        (Kind::String, Value::String(text)) => Ok(text.as_str()),
        _ => Err(invalid(format!(
            "`{}` tests strings, and `{}` holds {} or it is tested against {value:?}",
            op.text(),
            text(),
            path.leaf.describe()
        ))),
    };

    let value = |at: usize| operands.get(at);

    Ok(match (op, operands.len()) {
        (Op::IsNull, 0) => Test::IsNull,
        (Op::Between, 2) => Test::Between(check(value(0)?)?, check(value(1)?)?),
        (Op::In, len) => {
            let mut values = (0..len)
                .map(|at| check(value(at)?))
                .collect::<Result<Vec<_>>>()?;

            values.sort_by(|a, b| key::compare(a, b));
            values.dedup_by(|a, b| key::compare(a, b).is_eq());

            Test::In(values)
        }
        (Op::Contains, 1) if path.list_leaf => Test::Element(check(value(0)?)?),
        (Op::Contains, 1) => Test::Substring(string(value(0)?)?),
        (Op::StartsWith, 1) => Test::StartsWith(string(value(0)?)?),
        (Op::EndsWith, 1) => Test::EndsWith(string(value(0)?)?),
        (Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge, 1) => {
            Test::Compare(op, check(value(0)?)?)
        }
        _ => {
            return Err(invalid(format!(
                "`{}` on `{}` has the wrong number of values",
                op.text(),
                text()
            )));
        }
    })
}

/// Where a scalar kind's encodings lie: from its first tag up to, but not
/// including, the next.
fn type_range(kind: &Kind) -> Range {
    let (first, next) = match kind {
        Kind::Bool => key::BOOL_TAGS,
        Kind::Int => key::INT_TAGS,
        Kind::Float => key::FLOAT_TAGS,
        Kind::String => key::STRING_TAGS,
        _ => key::BYTES_TAGS,
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

/// A term of the filter that the primary key or an index answers, borrowed
/// from the term, so that choosing among them copies no value.
struct Candidate<'s, 'c> {
    term: usize,
    field: &'s FieldDef,
    /// The type of the field's values, a link's being its target's key type.
    leaf: &'c Kind,
    source: Source,
    index: Option<&'s IndexDef>,
    lookup: Lookup,
    /// The values looked up, for `Equal` and `Several`.
    values: &'c [Cow<'s, Value>],
    /// The range of values, for `Range`, as bounds on the values.
    low: Bound<&'c Value>,
    high: Bound<&'c Value>,
    prefix: Option<&'c str>,
}

/// The value an `IS NULL` on an index looks up.
const NULL: &[Cow<'static, Value>] = &[Cow::Borrowed(&Value::Null)];

impl Candidate<'_, '_> {
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

fn candidate<'s, 'c>(
    collection: &'s CollectionDef,
    term: usize,
    cond: &'c Cond<'s>,
) -> Option<Candidate<'s, 'c>> {
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
        leaf: &path.leaf,
        source,
        index,
        lookup: Lookup::Range,
        values: &[],
        low: Bound::Unbounded,
        high: Bound::Unbounded,
        prefix: None,
    };

    match test {
        Test::Compare(Op::Eq, value) | Test::Element(value) => {
            found.lookup = Lookup::Equal;
            found.values = std::slice::from_ref(value);
        }
        Test::IsNull if source != Source::Key => {
            found.lookup = Lookup::Equal;
            found.values = NULL;
        }
        Test::In(values) => {
            found.lookup = Lookup::Several;
            found.values = values;
        }
        Test::Compare(Op::Lt, value) => found.high = Bound::Excluded(value),
        Test::Compare(Op::Le, value) => found.high = Bound::Included(value),
        Test::Compare(Op::Gt, value) => found.low = Bound::Excluded(value),
        Test::Compare(Op::Ge, value) => found.low = Bound::Included(value),
        Test::Between(low, high) => {
            found.low = Bound::Included(low);
            found.high = Bound::Included(high);
        }
        Test::StartsWith(prefix) => found.prefix = Some(prefix),
        _ => return None,
    }

    Some(found)
}

/// The tighter of two lower bounds on values.
fn higher_low<'v>(a: Bound<&'v Value>, b: Bound<&'v Value>) -> Bound<&'v Value> {
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
fn lower_high<'v>(a: Bound<&'v Value>, b: Bound<&'v Value>) -> Bound<&'v Value> {
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
    low: Bound<&Value>,
    high: Bound<&Value>,
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
    // The terms are borrowed and the candidates made again where they are
    // needed, rather than gathered: a lookup by one value planned both into
    // vectors of their own every time it ran.
    let terms: &[Cond<'s>] = match filter {
        Some(Cond::And(terms)) => terms,
        Some(cond) => std::slice::from_ref(cond),
        None => &[],
    };
    let candidates = || {
        terms
            .iter()
            .enumerate()
            .filter_map(|(position, cond)| candidate(collection, position, cond))
    };
    // The field the query sorts by alone, and whether descending.
    let sorted_by = match sort {
        [(path, descending)] => path.direct.map(|field| (field.id, *descending)),
        _ => None,
    };
    let best = candidates().min_by_key(|candidate| (candidate.rank(), candidate.term));

    let Some(best) = best else {
        return unfiltered(collection, sort, sorted_by);
    };
    let backward =
        sorted_by.is_some_and(|(field, descending)| field == best.field.id && descending);
    let follows_sort = sorted_by.is_some_and(|(field, _)| field == best.field.id);
    let list = matches!(best.field.kind, Kind::List(_));

    match (best.lookup, best.source) {
        (Lookup::Equal | Lookup::Several, Source::Key) => {
            let keys: Vec<Vec<u8>> = best.values.iter().map(|value| encoded(value)).collect();

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
                    values: true,
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
            let mut prefix: Option<&str> = None;
            let mut consumed = Vec::new();

            for candidate in candidates() {
                let merges =
                    candidate.term == best.term || (!list && candidate.field.id == best.field.id);

                if candidate.lookup != Lookup::Range || !merges {
                    continue;
                }

                if candidate.prefix.is_some() && prefix.is_some() {
                    continue;
                }

                low = higher_low(low, candidate.low);
                high = lower_high(high, candidate.high);
                prefix = prefix.or(candidate.prefix);
                consumed.push(candidate.term);
            }

            let entries = source != Source::Key;
            let range = range(best.leaf, low, high, prefix, entries);

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
                        values: false,
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
                        values: false,
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
