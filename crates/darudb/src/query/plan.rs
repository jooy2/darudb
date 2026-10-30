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
//!
//! Planning also finds the terms that indexes answer alone, a link's
//! included, as parts whose sets of keys a run may gather and combine
//! rather than test each object read against them (`design/objects.md`,
//! "Sets of keys"). Whether that is cheaper depends on how many entries the
//! indexes hold, which only a run can count, so the run decides.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::ops::{Bound, Deref};
use std::rc::Rc;

use super::aggregate::Aggregate;
use super::fold;
use super::ir::{Expr, Ir, MAX_DEPTH, Op, Operand, SortKey};
use super::keys::KeySet;
use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::key;
use crate::format::object::schema::{
    CollectionDef, FieldDef, Fields, IndexDef, Kind, StoredSchema, path_fields,
};

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
    /// The collection a link field the path ends at links to, whose keys are
    /// the link's values.
    pub(crate) link: Option<&'s CollectionDef>,
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
    /// `CONTAINS` on a string: a searcher for it, made once for the query,
    /// and boxed, since it is several times the size of any other test.
    Substring(Box<memchr::memmem::Finder<'s>>),
    StartsWith(&'s str),
    EndsWith(&'s str),
    IsNull,
    /// `==` ignoring case, or `CONTAINS` on a list of strings: the value,
    /// folded.
    EqFold(String),
    /// `!=` ignoring case.
    NeFold(String),
    /// `IN` ignoring case: the values, folded.
    InFold(Vec<String>),
    /// `CONTAINS` on a string, ignoring case.
    SubstringFold(Box<fold::Search>),
    /// `STARTSWITH` ignoring case: the text, folded.
    StartsWithFold(String),
    /// `ENDSWITH` ignoring case.
    EndsWithFold(String),
}

/// A filter, resolved against the schema.
#[derive(Debug)]
pub(crate) enum Cond<'s> {
    And(Vec<Cond<'s>>),
    Or(Vec<Cond<'s>>),
    Not(Box<Cond<'s>>),
    Test {
        path: Resolved<'s>,
        test: Test<'s>,
    },
    /// Some object of another collection links to this one and meets its
    /// own filter. Shared with the part it is, which gathers the same keys.
    Linked(Rc<Linked<'s>>),
}

/// A test of the objects that link to an object, `ANY` in the query
/// language: the objects of `collection` its own `plan` finds, and the path
/// of their link to the collection tested, whose values, gathered once into
/// `keys` when the run first needs them, are the keys of the objects that
/// hold.
#[derive(Debug)]
pub(crate) struct Linked<'s> {
    /// The collection tested, whose objects are linked to.
    pub(crate) linked: &'s CollectionDef,
    /// Its primary key, which each object tested is looked for by.
    pub(crate) key: &'s FieldDef,
    /// The path of the link in the objects that link.
    pub(crate) path: Resolved<'s>,
    /// The objects of the linking collection that the filter finds.
    pub(crate) plan: Plan<'s>,
    /// The encoded keys their links name, once gathered.
    pub(crate) keys: OnceCell<KeySet>,
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

/// A part of the filter that indexes answer alone: the primary keys of the
/// objects that meet it can be gathered from the records' keys and the
/// indexes' entries, without reading an object (`design/objects.md`, "Sets
/// of keys").
#[derive(Debug)]
pub(crate) enum Part<'s> {
    /// The keys of the collection's records within these ranges of keys.
    Records(Vec<Range>),
    /// These keys, of the records that are there.
    Keys(Vec<Vec<u8>>),
    /// The objects an index names within these ranges of its entries.
    Index {
        index: &'s IndexDef,
        ranges: Vec<Range>,
    },
    /// The objects whose link field names, through its index, an object of
    /// `target` that `inner` finds.
    Linked {
        index: &'s IndexDef,
        target: &'s CollectionDef,
        inner: Box<Part<'s>>,
    },
    And(Vec<Part<'s>>),
    Or(Vec<Part<'s>>),
    /// The objects the links of a test of the objects that link to them
    /// name, those that are there.
    Backlinked(Rc<Linked<'s>>),
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
    /// The field of the collection itself that a walk over every record
    /// reads first in each: the first the filter tests, or else the first
    /// the sort reads. The walk marks where it lies in each record of a
    /// leaf, and the marks are kept beside the page cache, so that a query
    /// that walks the leaf again finds it there.
    pub(crate) marked: Option<&'s FieldDef>,
    /// The terms of `filter` that indexes answer alone, by their position
    /// among its terms, a filter that is not an `AND` being its only term.
    pub(crate) parts: Vec<(usize, Part<'s>)>,
    /// What the access answers of the query's filter, as a part, when it
    /// answers a term: a run that walks something else gathers it instead.
    pub(crate) access_part: Option<Part<'s>>,
    /// The walk in the order of the query's sort, when the access is not
    /// one: a run that gathered a set of keys may walk it and take the keys
    /// in the set, and stop at the limit.
    pub(crate) sort_walk: Option<Access<'s>>,
}

/// The field [`Plan::marked`] names for `filter` and `sort`.
fn marked<'s>(filter: Option<&Cond<'s>>, sort: &[(Resolved<'s>, bool)]) -> Option<&'s FieldDef> {
    fn first<'s>(cond: &Cond<'s>) -> Option<&'s FieldDef> {
        match cond {
            Cond::And(terms) | Cond::Or(terms) => terms.first().and_then(first),
            Cond::Not(term) => first(term),
            Cond::Test { path, .. } => path.direct,
            Cond::Linked(_) => None,
        }
    }

    match filter {
        Some(filter) => first(filter),
        None => sort.first().and_then(|(path, _)| path.direct),
    }
}

/// Checks `ir` against `collection` of `schema`, with `parameters` for its
/// parameters, and plans it.
pub(crate) fn plan<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    ir: &'s Ir,
    parameters: &'s [Value],
) -> Result<Plan<'s>> {
    let query = Parts {
        filter: ir.filter.as_ref(),
        sort: &ir.sort,
        offset: ir.offset,
        limit: ir.limit,
    };

    plan_parts(schema, collection, query, parameters, 1)
}

/// The parts of a query to plan: those of an IR, or the filter alone of the
/// objects that link to another, which a test of them plans.
#[derive(Clone, Copy)]
struct Parts<'s> {
    filter: Option<&'s Expr>,
    sort: &'s [SortKey],
    offset: u64,
    limit: Option<u64>,
}

/// [`plan`] of `query`, whose filter nests from `depth`.
fn plan_parts<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    query: Parts<'s>,
    parameters: &'s [Value],
    depth: usize,
) -> Result<Plan<'s>> {
    let (filter, sort) = resolve(schema, collection, query, parameters, depth)?;
    let (access, consumed, ordered) = choose(collection, filter.as_ref(), &sort);
    // The parts of the terms the access leaves, by their positions in what
    // is left of the filter.
    let parts = terms_of(filter.as_ref())
        .iter()
        .enumerate()
        .filter(|(position, _)| !consumed.contains(position))
        .enumerate()
        .filter_map(|(left, (_, term))| part(collection, term, 0).map(|part| (left, part)))
        .collect();
    let access_part = (!consumed.is_empty())
        .then(|| access_part(&access))
        .flatten();
    let sort_walk = if ordered || sort.is_empty() {
        None
    } else {
        let sorted_by = match sort.as_slice() {
            [(path, descending)] => path.direct.map(|field| (field.id, *descending)),
            _ => None,
        };

        match unfiltered(collection, &sort, sorted_by) {
            (walk, _, true) => Some(walk),
            _ => None,
        }
    };
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
        marked: marked(filter.as_ref(), &sort),
        collection,
        filter,
        sort,
        offset: query.offset,
        limit: query.limit,
        access,
        ordered,
        exact,
        parts,
        access_part,
        sort_walk,
    })
}

/// The terms of a filter: those of its top-level `AND`, or the filter alone.
pub(crate) fn terms_of<'f, 's>(filter: Option<&'f Cond<'s>>) -> &'f [Cond<'s>] {
    match filter {
        Some(Cond::And(terms)) => terms,
        Some(cond) => std::slice::from_ref(cond),
        None => &[],
    }
}

/// What `access` answers, as a part.
fn access_part<'s>(access: &Access<'s>) -> Option<Part<'s>> {
    Some(match access {
        Access::Records { range, .. } => Part::Records(vec![range.clone()]),
        Access::Keys { keys, .. } => Part::Keys(keys.clone()),
        Access::Index { index, ranges, .. } => Part::Index {
            index,
            ranges: ranges.clone(),
        },
    })
}

/// The part `cond` is, when indexes answer it alone. A part within an `AND`
/// or an `OR` answers it only with every other term's.
fn part<'s>(collection: &'s CollectionDef, cond: &Cond<'s>, depth: usize) -> Option<Part<'s>> {
    if depth > MAX_DEPTH {
        return None;
    }

    match cond {
        Cond::Test { path, test } => path_part(collection, &path.steps, &path.leaf, test),
        Cond::And(terms) => terms
            .iter()
            .map(|term| part(collection, term, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Part::And),
        Cond::Or(terms) => terms
            .iter()
            .map(|term| part(collection, term, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Part::Or),
        Cond::Not(_) => None,
        Cond::Linked(linked) => Some(Part::Backlinked(Rc::clone(linked))),
    }
}

/// The part a test of the values at `steps` from an object of `collection`
/// is, when indexes answer it: a field of the collection, or a link field
/// with an index, followed to a part of the linked collection.
fn path_part<'s>(
    collection: &'s CollectionDef,
    steps: &[Step<'s>],
    leaf: &Kind,
    test: &Test<'s>,
) -> Option<Part<'s>> {
    let Some((Step::Field(name), rest)) = steps.split_first() else {
        return None;
    };
    let field = collection.fields.by_name(name)?;
    // Each element of a list has an entry in the list's index, so a path into
    // a list's elements is a lookup of the list's index.
    let rest = match rest.split_first() {
        Some((Step::Expand, rest)) => rest,
        _ => rest,
    };

    match rest {
        [] => direct_part(collection, field, leaf, test),
        [Step::Follow(target), rest @ ..] => {
            // A link to an object that is not there reads as null, and only
            // `IS NULL` holds for it, which no entry of the linked
            // collection's index says.
            if matches!(test, Test::IsNull) {
                return None;
            }

            let index = collection
                .indexes
                .iter()
                .find(|index| index.is_on(field.id))?;
            let inner = path_part(target, rest, leaf, test)?;

            Some(Part::Linked {
                index,
                target,
                inner: Box::new(inner),
            })
        }
        _ => None,
    }
}

/// The part a test of `field`, a field of `collection` itself, is, when its
/// index or the primary key answers it: the ranges the access would walk
/// for it alone.
fn direct_part<'s>(
    collection: &'s CollectionDef,
    field: &'s FieldDef,
    leaf: &Kind,
    test: &Test<'s>,
) -> Option<Part<'s>> {
    let found = candidate_on(collection, 0, field, leaf, test)?;

    Some(match (found.lookup, found.source, found.index) {
        (Lookup::Equal | Lookup::Several, Source::Key, _) => {
            Part::Keys(found.values.iter().map(|value| encoded(value)).collect())
        }
        (Lookup::Equal | Lookup::Several, _, Some(index)) => Part::Index {
            index,
            ranges: found
                .values
                .iter()
                .map(|value| {
                    let bytes = encoded(value);
                    let end = after(&bytes);

                    (Bound::Included(bytes), end)
                })
                .collect(),
        },
        (Lookup::Range, Source::Key, _) => Part::Records(vec![range(
            found.leaf,
            found.low,
            found.high,
            found.prefix,
            false,
        )]),
        (Lookup::Range, _, Some(index)) => Part::Index {
            index,
            ranges: vec![range(found.leaf, found.low, found.high, found.prefix, true)],
        },
        (_, _, None) => return None,
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
    let query = Parts {
        filter: ir.filter.as_ref(),
        sort: &ir.sort,
        offset: ir.offset,
        limit: ir.limit,
    };
    let (filter, sort) = resolve(schema, collection, query, parameters, 1)?;

    Ok(Plan {
        marked: marked(filter.as_ref(), &sort),
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
        parts: Vec::new(),
        access_part: None,
        sort_walk: None,
    })
}

/// The filter and the sort of `ir`, resolved against the schema.
#[expect(clippy::type_complexity, reason = "a pair of the two parts of a query")]
fn resolve<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    query: Parts<'s>,
    parameters: &'s [Value],
    depth: usize,
) -> Result<(Option<Cond<'s>>, Vec<(Resolved<'s>, bool)>)> {
    let resolver = Resolver {
        schema,
        collection,
        parameters,
    };
    let filter = query
        .filter
        .map(|expr| resolver.cond(expr, depth))
        .transpose()?;
    let sort = query
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

/// The path `function` reads in `collection`, resolved: it has to read one
/// value of each object, not a list or through one, and of a type the
/// function takes.
pub(crate) fn aggregated<'s>(
    schema: &'s StoredSchema,
    collection: &'s CollectionDef,
    path: &[String],
    function: Aggregate,
) -> Result<Resolved<'s>> {
    let resolver = Resolver {
        schema,
        collection,
        parameters: &[],
    };
    let resolved = resolver.path(path)?;
    let text = || path.join(".");

    if resolved.many || resolved.list_leaf {
        return Err(invalid(format!(
            "`{}` holds several values, and `{}` reads one value of each object",
            text(),
            function.text()
        )));
    }

    if resolved.link.is_some() {
        return Err(invalid(format!(
            "`{}` is a link, and `{}` reads a field of the object it links to, such as `{}.name`",
            text(),
            function.text(),
            text()
        )));
    }

    let takes = match function {
        Aggregate::Sum | Aggregate::Avg => matches!(resolved.leaf, Kind::Int | Kind::Float),
        Aggregate::Min | Aggregate::Max => matches!(
            resolved.leaf,
            Kind::Int | Kind::Float | Kind::String | Kind::Bytes | Kind::Date
        ),
    };

    if !takes {
        return Err(invalid(format!(
            "`{}` holds {}, which `{}` does not read",
            text(),
            resolved.leaf.describe(),
            function.text()
        )));
    }

    Ok(resolved)
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
            Expr::Linked {
                collection,
                path,
                filter,
            } => Cond::Linked(Rc::new(self.linked(
                collection,
                path,
                filter.as_deref(),
                depth,
            )?)),
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
                match (op.plain(), operands.single()?) {
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

    /// The test of the objects of `collection` whose link at `path` names an
    /// object of this collection and that meet `filter`.
    fn linked(
        &self,
        collection: &'s str,
        path: &'s [String],
        filter: Option<&'s Expr>,
        depth: usize,
    ) -> Result<Linked<'s>> {
        let text = || format!("ANY {collection}.{}", path.join("."));
        let linking = self.schema.collection(collection).ok_or_else(|| {
            invalid(format!(
                "`{}` names `{collection}`, which is not a collection",
                text()
            ))
        })?;
        let resolved = Resolver {
            collection: linking,
            ..*self
        }
        .path(path)?;

        if resolved.link.map(|target| target.id) != Some(self.collection.id) {
            return Err(invalid(format!(
                "`{}` is not a link of `{collection}` to `{}`",
                text(),
                self.collection.name
            )));
        }

        let key = self
            .collection
            .key_field()
            .ok_or_else(|| invalid(format!("`{}` has no primary key", self.collection.name)))?;
        let query = Parts {
            filter,
            sort: &[],
            offset: 0,
            limit: None,
        };

        Ok(Linked {
            linked: self.collection,
            key,
            path: resolved,
            plan: plan_parts(self.schema, linking, query, self.parameters, depth + 1)?,
            keys: OnceCell::new(),
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
                            link: Some(target),
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
                        link: None,
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
            | (Kind::Date, Value::Date(_))
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

    if op.folds() {
        if path.leaf != Kind::String || path.link.is_some() {
            return Err(invalid(format!(
                "`IGNORE CASE` compares strings, and `{}` holds {}",
                text(),
                path.leaf.describe()
            )));
        }

        let folded = |at: usize| value(at).and_then(&string).map(fold::folded);

        return Ok(match (op, operands.len()) {
            (Op::EqFold, 1) => Test::EqFold(folded(0)?),
            (Op::ContainsFold, 1) if path.list_leaf => Test::EqFold(folded(0)?),
            (Op::NeFold, 1) => Test::NeFold(folded(0)?),
            (Op::InFold, len) => {
                let mut values = (0..len).map(folded).collect::<Result<Vec<_>>>()?;

                values.sort_unstable();
                values.dedup();

                Test::InFold(values)
            }
            (Op::ContainsFold, 1) => {
                Test::SubstringFold(Box::new(fold::Search::new(string(value(0)?)?)))
            }
            (Op::StartsWithFold, 1) => Test::StartsWithFold(folded(0)?),
            (Op::EndsWithFold, 1) => Test::EndsWithFold(folded(0)?),
            _ => {
                return Err(invalid(format!(
                    "`{}` on `{}` has the wrong number of values",
                    op.text(),
                    text()
                )));
            }
        });
    }

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
        (Op::Contains, 1) => {
            Test::Substring(Box::new(memchr::memmem::Finder::new(string(value(0)?)?)))
        }
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
        // A date's key is its microseconds' int.
        Kind::Int | Kind::Date => key::INT_TAGS,
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

    candidate_on(collection, term, path.direct?, &path.leaf, test)
}

/// [`candidate`] for a test of `field`, a field of `collection` itself
/// whose values are of type `leaf`.
fn candidate_on<'s, 'c>(
    collection: &'s CollectionDef,
    term: usize,
    field: &'s FieldDef,
    leaf: &'c Kind,
    test: &'c Test<'s>,
) -> Option<Candidate<'s, 'c>> {
    let (source, index) = if field.id == collection.key {
        (Source::Key, None)
    } else {
        let index = collection
            .indexes
            .iter()
            .find(|index| index.is_on(field.id))?;

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
        leaf,
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

/// The most combinations of values an access through an index of several
/// components looks up, from the `IN`s of its components: beyond, the
/// components after the `IN` that makes them are left to test.
const COMBINATIONS: usize = 64;

/// An access through an index that is not on one field of the collection
/// itself, one of several components or one inside an embedded object, the
/// positions of the terms it answers, whether it delivers the query's order,
/// and how good it is: the lookup's kind as [`Candidate::rank`] ranks it,
/// then how much of the index it narrows, more first.
struct General<'s> {
    access: Access<'s>,
    consumed: Vec<usize>,
    ordered: bool,
    rank: (u8, i32),
}

/// The steps a path of field ids from `fields` takes, as a query's path
/// resolves them: a field each, and each element of a list after a list.
fn steps_of<'s>(fields: &'s Fields, path: &[u64]) -> Option<Vec<Step<'s>>> {
    let mut steps = Vec::new();

    for field in path_fields(fields, path)? {
        steps.push(Step::Field(&field.name));

        if matches!(field.kind, Kind::List(_)) {
            steps.push(Step::Expand);
        }
    }

    Some(steps)
}

/// Whether a query's path takes exactly `steps`.
fn same_steps(path: &[Step<'_>], steps: &[Step<'_>]) -> bool {
    path.len() == steps.len()
        && path.iter().zip(steps).all(|pair| match pair {
            (Step::Field(a), Step::Field(b)) => a == b,
            (Step::Expand, Step::Expand) => true,
            _ => false,
        })
}

/// The best access through an index of `collection` that is not on one of
/// its fields alone, for the top-level `terms` and `sort`: the components
/// from the first that terms test for equality, a range on the next, and an
/// order the components after them give.
fn general<'s>(
    collection: &'s CollectionDef,
    terms: &[Cond<'s>],
    sort: &[(Resolved<'s>, bool)],
) -> Option<General<'s>> {
    collection
        .indexes
        .iter()
        .filter(|index| index.field().is_none())
        .filter_map(|index| through(collection, index, terms, sort))
        .min_by_key(|found| found.rank)
}

/// The top-level terms that test the values at `component`: each one's
/// position, the type of the values, and the test.
fn tests_on<'t, 's>(
    terms: &'t [Cond<'s>],
    component: &'t [Step<'s>],
) -> impl Iterator<Item = (usize, &'t Kind, &'t Test<'s>)> + 't {
    terms
        .iter()
        .enumerate()
        .filter_map(move |(position, term)| match term {
            Cond::Test { path, test } if same_steps(&path.steps, component) => {
                Some((position, &path.leaf, test))
            }
            _ => None,
        })
}

/// [`general`] for `index`.
fn through<'s>(
    collection: &'s CollectionDef,
    index: &'s IndexDef,
    terms: &[Cond<'s>],
    sort: &[(Resolved<'s>, bool)],
) -> Option<General<'s>> {
    let components = index
        .components
        .iter()
        .map(|path| steps_of(&collection.fields, path))
        .collect::<Option<Vec<_>>>()?;
    let list = components
        .iter()
        .any(|steps| steps.iter().any(|step| matches!(step, Step::Expand)));
    let mut prefixes: Vec<Vec<u8>> = vec![Vec::new()];
    let mut consumed = Vec::new();
    let mut equal = 0;
    // Whether a component is looked up as null, which keys a unique index's
    // entry by the object's key too.
    let mut nulls = false;

    // The components from the first that a term tests for equality: each
    // value, or each of an `IN`'s, after each prefix so far.
    'components: for component in &components {
        for (position, _, test) in tests_on(terms, component) {
            let values: Vec<&Value> = match test {
                Test::Compare(Op::Eq, value) | Test::Element(value) => vec![value.as_ref()],
                Test::IsNull => vec![&Value::Null],
                Test::In(values) if prefixes.len() * values.len() <= COMBINATIONS => {
                    values.iter().map(AsRef::as_ref).collect()
                }
                _ => continue,
            };

            nulls |= values.iter().any(|value| value.is_null());
            prefixes = prefixes
                .iter()
                .flat_map(|prefix| {
                    values.iter().map(move |value| {
                        let mut bytes = prefix.clone();

                        bytes.extend_from_slice(&encoded(value));
                        bytes
                    })
                })
                .collect();
            consumed.push(position);
            equal += 1;

            continue 'components;
        }

        break;
    }

    // A range on the component after them, narrowed by every range term on
    // it, but one alone on a list, whose terms may hold for two elements.
    let mut ranged = false;
    let mut ranges: Vec<Range> = Vec::new();

    if let Some(component) = components.get(equal) {
        let mut low = Bound::Unbounded;
        let mut high = Bound::Unbounded;
        let mut prefix: Option<&str> = None;
        let mut leaf = None;

        for (position, kind, test) in tests_on(terms, component) {
            let (term_low, term_high, term_prefix) = match test {
                Test::Compare(Op::Lt, value) => {
                    (Bound::Unbounded, Bound::Excluded(value.as_ref()), None)
                }
                Test::Compare(Op::Le, value) => {
                    (Bound::Unbounded, Bound::Included(value.as_ref()), None)
                }
                Test::Compare(Op::Gt, value) => {
                    (Bound::Excluded(value.as_ref()), Bound::Unbounded, None)
                }
                Test::Compare(Op::Ge, value) => {
                    (Bound::Included(value.as_ref()), Bound::Unbounded, None)
                }
                Test::Between(a, b) => (
                    Bound::Included(a.as_ref()),
                    Bound::Included(b.as_ref()),
                    None,
                ),
                Test::StartsWith(text) if prefix.is_none() => {
                    (Bound::Unbounded, Bound::Unbounded, Some(*text))
                }
                _ => continue,
            };

            if ranged && list {
                break;
            }

            low = higher_low(low, term_low);
            high = lower_high(high, term_high);
            prefix = prefix.or(term_prefix);
            leaf = Some(kind);
            ranged = true;
            consumed.push(position);
        }

        if let Some(leaf) = leaf {
            let (start, end) = range(leaf, low, high, prefix, true);

            ranges = prefixes
                .iter()
                .map(|bytes| {
                    let start = match &start {
                        Bound::Included(value) => {
                            Bound::Included([bytes.as_slice(), value].concat())
                        }
                        Bound::Excluded(value) => {
                            Bound::Excluded([bytes.as_slice(), value].concat())
                        }
                        Bound::Unbounded => Bound::Included(bytes.clone()),
                    };
                    let end = match &end {
                        Bound::Included(value) => {
                            Bound::Included([bytes.as_slice(), value].concat())
                        }
                        Bound::Excluded(value) => {
                            Bound::Excluded([bytes.as_slice(), value].concat())
                        }
                        Bound::Unbounded => after(bytes),
                    };

                    (start, end)
                })
                .collect();
        }
    }

    if !ranged {
        ranges = prefixes
            .iter()
            .map(|bytes| (Bound::Included(bytes.clone()), after(bytes)))
            .collect();
    }

    let whole = equal == components.len();
    let one = prefixes.len() == 1;
    // The sort is the components after the equal ones, every one of them,
    // in order and all one way, which the index gives within one prefix,
    // with ties in primary key order. A sort that left a component out would
    // get its ties in that component's order.
    let follows = !list
        && one
        && !sort.is_empty()
        && equal + sort.len() == components.len()
        && sort.iter().enumerate().all(|(at, (path, descending))| {
            components
                .get(equal + at)
                .is_some_and(|component| same_steps(&path.steps, component))
                && *descending == sort[0].1
        });
    let ordered = follows || (sort.is_empty() && whole && one && !ranged && !list);
    let backward = follows && sort[0].1;
    let narrowed = i32::try_from(equal * 2 + usize::from(ranged)).unwrap_or(i32::MAX);
    let lookup = match (equal, ranged) {
        (0, false) if follows => 5,
        (0, false) => return None,
        (0, true) => 4,
        _ if whole && index.unique && !ranged && one && !nulls => 0,
        _ if whole && index.unique && !ranged => 1,
        // Each value of an `IN` narrows within the components before it, so
        // the ranges of several are as narrow as the one of an equality.
        _ => 2,
    };

    Some(General {
        access: Access::Index {
            index,
            ranges: if lookup == 5 {
                vec![(Bound::Unbounded, Bound::Unbounded)]
            } else {
                ranges
            },
            backward,
            repeats: list && !(whole && one && !ranged),
            values: whole && !ranged,
        },
        consumed,
        ordered,
        rank: (lookup, -narrowed),
    })
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
    let general = general(collection, terms, sort);

    // An index of several components, or inside an embedded object, goes
    // first when it narrows more than the best term of one field, and never
    // before the primary key.
    if let Some(general) = general {
        let beats = match &best {
            None => {
                general.rank.0 < 5
                    || !matches!(unfiltered(collection, sort, sorted_by), (_, _, true))
            }
            Some(best) if best.source == Source::Key => false,
            Some(best) => {
                let narrowed = if best.lookup == Lookup::Range { -1 } else { -2 };
                let rank = (best.rank().0, narrowed);
                // As narrow, the one that gives the query's order goes first.
                let ordered =
                    sort.is_empty() || sorted_by.is_some_and(|(field, _)| field == best.field.id);

                general.rank < rank || (general.rank == rank && general.ordered && !ordered)
            }
        };

        if beats {
            return (general.access, general.consumed, general.ordered);
        }
    }

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
            match collection.indexes.iter().find(|index| index.is_on(field)) {
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
