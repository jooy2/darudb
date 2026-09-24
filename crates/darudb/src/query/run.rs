//! Running a planned query: walking its access, testing each object read,
//! sorting, and skipping and stopping (`design/objects.md`, "Running a
//! query").

use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::Bound;

use super::ir::Op;
use super::plan::{Access, Cond, Plan, Range, Resolved, Step, Test};
use crate::error::Result;
use crate::format::object::codec::{self, FieldRef};
use crate::format::object::schema::{CollectionDef, IndexDef, Kind};
use crate::format::object::{Object, Value, key};
use crate::schema::objects::{self, Source, index_tree, records};

/// How many linked objects a query keeps once read, so that a filter or a
/// sort that follows the same link twice reads the object once.
const LINK_CACHE: usize = 4096;

/// Below this many objects kept for a sort with a limit, the kept objects
/// are not trimmed to the limit.
const TRIM_AT: usize = 256;

/// Linked objects already read, by collection id and encoded key.
type Links = HashMap<(u64, Vec<u8>), Option<Object>>;

/// A query's reads: its transaction, and the linked objects already read.
struct Reader<'a> {
    source: &'a dyn Source,
    links: RefCell<Links>,
}

impl Reader<'_> {
    /// Whether `test` holds for any value at the end of `steps` from
    /// `value`. A null anywhere on the way is a null at the end.
    fn any(
        &self,
        value: &Value,
        steps: &[Step<'_>],
        test: &mut dyn FnMut(ValueRef<'_>) -> bool,
    ) -> Result<bool> {
        let Some((step, rest)) = steps.split_first() else {
            return Ok(test(ValueRef::Value(value)));
        };

        match (step, value) {
            (Step::Field(name), Value::Object(object)) => {
                self.any(object.get(name).unwrap_or(&Value::Null), rest, test)
            }
            (Step::Expand, Value::List(elements)) => {
                for element in elements {
                    if self.any(element, rest, test)? {
                        return Ok(true);
                    }
                }

                Ok(false)
            }
            (Step::Follow(target), key) if !key.is_null() => {
                let linked = self.linked(target, key)?;

                match linked {
                    Some(object) => self.any(&Value::Object(object), rest, test),
                    None => self.any(&Value::Null, rest, test),
                }
            }
            _ => self.any(&Value::Null, rest, test),
        }
    }

    /// Whether `test` holds for any value at `path` of an object.
    fn any_in(
        &self,
        fields: &dyn Fields,
        path: &Resolved<'_>,
        test: &mut dyn FnMut(ValueRef<'_>) -> bool,
    ) -> Result<bool> {
        let Some((Step::Field(name), rest)) = path.steps.split_first() else {
            return Ok(false);
        };

        match fields.field(name)? {
            Current::Borrowed(ValueRef::Value(value)) => self.any(value, rest, test),
            Current::Borrowed(value) if rest.is_empty() => Ok(test(value)),
            Current::Borrowed(value) => self.any(&value.to_value(), rest, test),
            Current::Owned(value) => self.any(&value, rest, test),
        }
    }

    /// The object of `target` that `key` names, from the cache when it has
    /// been read before.
    fn linked(&self, target: &CollectionDef, key: &Value) -> Result<Option<Object>> {
        // Writes check a link's key against its target, so another type
        // here is damage.
        let encoded = objects::key_bytes(target, key).map_err(|_| {
            self.source.corrupted(format!(
                "a link to `{}` holds a key of another type",
                target.name
            ))
        })?;
        let cache_key = (target.id, encoded);

        if let Some(object) = self.links.borrow().get(&cache_key) {
            return Ok(object.clone());
        }

        let object = objects::get(self.source, target, key)?;
        let mut links = self.links.borrow_mut();

        if links.len() >= LINK_CACHE {
            links.clear();
        }

        links.insert(cache_key, object.clone());

        Ok(object)
    }

    fn holds(&self, cond: &Cond<'_>, fields: &dyn Fields) -> Result<bool> {
        match cond {
            Cond::And(terms) => {
                for term in terms {
                    if !self.holds(term, fields)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            Cond::Or(terms) => {
                for term in terms {
                    if self.holds(term, fields)? {
                        return Ok(true);
                    }
                }

                Ok(false)
            }
            Cond::Not(term) => Ok(!self.holds(term, fields)?),
            Cond::Test {
                path,
                test: Test::IsNull,
            } => self.any_in(fields, path, &mut |value| value.is_null()),
            Cond::Test { path, test } => self.any_in(fields, path, &mut |value| {
                !value.is_null() && passes(test, value)
            }),
        }
    }

    /// The value at a single-valued `path` of an object, for sorting.
    fn value_at(&self, fields: &dyn Fields, path: &Resolved<'_>) -> Result<Value> {
        let mut found = Value::Null;

        self.any_in(fields, path, &mut |value| {
            found = value.to_value();
            true
        })?;

        Ok(found)
    }
}

/// A value a filter or a sort reads: borrowed from a record where it is a
/// scalar there, so that testing it copies nothing, or from a value.
#[derive(Debug, Clone, Copy)]
enum ValueRef<'a> {
    Value(&'a Value),
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(&'a str),
    Bytes(&'a [u8]),
}

impl ValueRef<'_> {
    fn is_null(self) -> bool {
        matches!(self, ValueRef::Null | ValueRef::Value(Value::Null))
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            ValueRef::String(text) => Some(text),
            ValueRef::Value(value) => value.as_str(),
            _ => None,
        }
    }

    fn to_value(self) -> Value {
        match self {
            ValueRef::Value(value) => value.clone(),
            ValueRef::Null => Value::Null,
            ValueRef::Bool(value) => Value::Bool(value),
            ValueRef::Int(value) => Value::Int(value),
            ValueRef::Float(value) => Value::Float(value),
            ValueRef::String(value) => Value::String(value.to_owned()),
            ValueRef::Bytes(value) => Value::Bytes(value.to_vec()),
        }
    }

    /// The order of this value and `other`, as [`key::compare`] gives it.
    fn compare(self, other: &Value) -> Ordering {
        match (self, other) {
            (ValueRef::Value(value), _) => key::compare(value, other),
            (ValueRef::String(value), Value::String(other)) => {
                value.as_bytes().cmp(other.as_bytes())
            }
            (ValueRef::Bytes(value), Value::Bytes(other)) => value.cmp(other.as_slice()),
            // Values of two types are ordered by their types alone, as an
            // empty value of the same type is, which costs no allocation.
            (ValueRef::String(_), _) => key::compare(&Value::String(String::new()), other),
            (ValueRef::Bytes(_), _) => key::compare(&Value::Bytes(Vec::new()), other),
            (ValueRef::Null, _) => key::compare(&Value::Null, other),
            (ValueRef::Bool(value), _) => key::compare(&Value::Bool(value), other),
            (ValueRef::Int(value), _) => key::compare(&Value::Int(value), other),
            (ValueRef::Float(value), _) => key::compare(&Value::Float(value), other),
        }
    }
}

/// A field's value as [`Fields::field`] gives it.
enum Current<'a> {
    Borrowed(ValueRef<'a>),
    Owned(Value),
}

/// The fields of an object, as a filter and a sort read them.
trait Fields {
    /// The value of the collection's field `name`.
    fn field(&self, name: &str) -> Result<Current<'_>>;
}

impl Fields for Object {
    fn field(&self, name: &str) -> Result<Current<'_>> {
        Ok(Current::Borrowed(
            self.get(name).map_or(ValueRef::Null, ValueRef::Value),
        ))
    }
}

/// An object's record, whose fields are found in it one at a time: nothing
/// is decoded but the fields read, and nothing copied but those of them that
/// are not scalars.
struct View<'a> {
    source: &'a dyn Source,
    collection: &'a CollectionDef,
    record: &'a [u8],
}

impl Fields for View<'_> {
    fn field(&self, name: &str) -> Result<Current<'_>> {
        let damaged = |reason: &str| {
            self.source
                .corrupted(format!("an object of `{}`: {reason}", self.collection.name))
        };
        let Some(field) = self.collection.fields.by_name(name) else {
            return Ok(Current::Borrowed(ValueRef::Null));
        };
        let found = codec::find_field(self.record, field.id).map_err(damaged)?;

        Ok(match (found, &field.kind) {
            (None, _) => match &field.default {
                Some(default) => Current::Borrowed(ValueRef::Value(default)),
                None if field.optional => Current::Borrowed(ValueRef::Null),
                None => return Err(damaged("a record lacks a required field")),
            },
            (Some(FieldRef::Bool(value)), Kind::Bool) => Current::Borrowed(ValueRef::Bool(value)),
            (Some(FieldRef::Int(value)), Kind::Int) => Current::Borrowed(ValueRef::Int(value)),
            (Some(FieldRef::Float(value)), Kind::Float) => {
                Current::Borrowed(ValueRef::Float(value))
            }
            (Some(FieldRef::String(value)), Kind::String) => {
                Current::Borrowed(ValueRef::String(value))
            }
            (Some(FieldRef::Bytes(value)), Kind::Bytes) => {
                Current::Borrowed(ValueRef::Bytes(value))
            }
            (Some(FieldRef::Encoded(bytes)), kind) => {
                Current::Owned(codec::value_of(bytes, kind).map_err(damaged)?)
            }
            (Some(_), _) => {
                return Err(damaged(
                    "a record holds a value of another type than its field",
                ));
            }
        })
    }
}

/// Whether `test` holds for the value `value`, which is not null.
fn passes(test: &Test, value: ValueRef<'_>) -> bool {
    match test {
        Test::Compare(op, other) => {
            let order = value.compare(other);

            match op {
                Op::Eq => order.is_eq(),
                Op::Ne => order.is_ne(),
                Op::Lt => order.is_lt(),
                Op::Le => order.is_le(),
                Op::Gt => order.is_gt(),
                _ => order.is_ge(),
            }
        }
        Test::Between(low, high) => value.compare(low).is_ge() && value.compare(high).is_le(),
        Test::In(values) => values
            .binary_search_by(|other| value.compare(other).reverse())
            .is_ok(),
        Test::Element(other) => value.compare(other).is_eq(),
        Test::Substring(text) => value.as_str().is_some_and(|value| value.contains(*text)),
        Test::StartsWith(text) => value.as_str().is_some_and(|value| value.starts_with(*text)),
        Test::EndsWith(text) => value.as_str().is_some_and(|value| value.ends_with(*text)),
        Test::IsNull => value.is_null(),
    }
}

/// What [`walk`] gives each object to: its primary key and, when the walk
/// read it, its record, both borrowed. It returns whether to stop.
type Visit<'v> = dyn FnMut(&[u8], Option<&[u8]>) -> Result<bool> + 'v;

/// Walks the keys the access names, in its order, reading each object once.
/// `visit` gets a primary key and, when the walk read it, the record, both
/// borrowed; it returns whether to stop.
fn walk(source: &dyn Source, plan: &Plan<'_>, visit: &mut Visit<'_>) -> Result<()> {
    let records = records(plan.collection.id);

    match &plan.access {
        Access::Records { range, backward } => {
            source
                .range_in(&records, as_ref(&range.0), as_ref(&range.1), *backward)?
                .for_each(&mut |key, record| visit(key, Some(record)))?;
        }
        Access::Keys { keys, backward } => {
            for at in 0..keys.len() {
                let key = &keys[in_order(at, keys.len(), *backward)];

                if let Some(record) = source.get_in(&records, key)? {
                    if visit(key, Some(&record))? {
                        break;
                    }
                }
            }
        }
        Access::Index {
            index,
            ranges,
            backward,
            repeats,
            values,
        } => {
            let tree = index_tree(index.id);
            let mut seen = HashSet::new();

            let mut give = |key: &[u8], seen: &mut HashSet<Vec<u8>>| -> Result<bool> {
                if *repeats && !seen.insert(key.to_vec()) {
                    return Ok(false);
                }

                visit(key, None)
            };

            for at in 0..ranges.len() {
                let range = &ranges[in_order(at, ranges.len(), *backward)];
                let stop = if let Some(value) = unique_value(index, *values, range) {
                    // One value of a unique index is one entry, whose key is
                    // the value and whose value is the object's key: looked
                    // up, not walked.
                    match source.get_in(&tree, value)? {
                        Some(key) => give(&key, &mut seen)?,
                        None => false,
                    }
                } else if *backward {
                    walk_back(source, index, &tree, range, &mut |key| give(key, &mut seen))?
                } else {
                    walk_entries(source, index, &tree, range, &mut |key| give(key, &mut seen))?
                };

                if stop {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// The position of the `at`th of `len` things taken forwards, or backwards.
fn in_order(at: usize, len: usize, backward: bool) -> usize {
    if backward { len - 1 - at } else { at }
}

/// The value `range` holds, if it is a range of one value, as `values` says,
/// of a unique index, and the value is not null, which any number of objects
/// may hold.
fn unique_value<'r>(index: &IndexDef, values: bool, range: &'r Range) -> Option<&'r [u8]> {
    match range {
        (Bound::Included(value), _)
            if values && index.unique && value.first() != Some(&key::NULL) =>
        {
            Some(value)
        }
        _ => None,
    }
}

/// How many objects of one value a backward walk holds back before it goes
/// to the value's first entry and walks its objects forwards instead.
///
/// Going there costs one more descent of the index, about what holding back
/// 15 to 25 entries costs, and the entries held back for a value that turns
/// out larger are read for nothing. Near that point, a value of any size
/// costs at most about twice what it would if its size were known: 64, the
/// size before, spent more on each large value than the descent it saved.
const GROUP: usize = 16;

/// The value an index entry holds and the primary key it names, borrowed
/// from the entry and the value stored with it.
fn entry_parts<'e>(
    source: &dyn Source,
    index: &IndexDef,
    entry: &'e [u8],
    stored: &'e [u8],
) -> Result<(&'e [u8], &'e [u8])> {
    let used = key::length(entry)
        .map_err(|reason| source.corrupted(format!("an entry of index {}: {reason}", index.id)))?;
    let (value, key) = entry.split_at(used);

    // A unique index keeps the key in the entry's value, and after the
    // value only where values repeat: null.
    Ok((value, if index.unique { stored } else { key }))
}

/// Gives the primary keys of the entries of `range`, forwards, until `give`
/// says to stop, and returns whether it did.
fn walk_entries(
    source: &dyn Source,
    index: &IndexDef,
    tree: &str,
    range: &Range,
    give: &mut dyn FnMut(&[u8]) -> Result<bool>,
) -> Result<bool> {
    let mut stopped = false;

    source
        .range_in(tree, as_ref(&range.0), as_ref(&range.1), false)?
        .for_each(&mut |entry, stored| {
            let (_, key) = entry_parts(source, index, entry, stored)?;

            stopped = give(key)?;

            Ok(stopped)
        })?;

    Ok(stopped)
}

/// Walks a range of an index backwards, giving each value's objects in
/// ascending key order, which is the order for ties.
///
/// Walking backwards, one value's objects come in descending key order, so
/// they are held back and given reversed once the value changes. A value
/// with more than [`GROUP`] objects is walked forwards from its first entry
/// instead, so that a query that stops after a few objects does not read a
/// value's thousands: the entries are `value || key`, and the bounds of a
/// range never cut a value's entries apart.
fn walk_back(
    source: &dyn Source,
    index: &IndexDef,
    tree: &str,
    range: &Range,
    give: &mut dyn FnMut(&[u8]) -> Result<bool>,
) -> Result<bool> {
    let mut high = range.1.clone();
    // The keys of the value held back, one after another, and where each
    // ends: one buffer for all of them rather than one for each.
    let mut held: Vec<u8> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut value: Vec<u8> = Vec::new();

    loop {
        let mut stopped = false;
        let mut large = false;

        held.clear();
        ends.clear();
        value.clear();

        source
            .range_in(tree, as_ref(&range.0), as_ref(&high), true)?
            .for_each(&mut |entry, stored| {
                let (entry, key) = entry_parts(source, index, entry, stored)?;

                if entry != value.as_slice() {
                    if give_held(&mut held, &mut ends, give)? {
                        stopped = true;

                        return Ok(true);
                    }

                    value.clear();
                    value.extend_from_slice(entry);
                }

                held.extend_from_slice(key);
                ends.push(held.len());
                large = ends.len() >= GROUP;

                Ok(large)
            })?;

        if stopped {
            return Ok(true);
        }

        if !large {
            return give_held(&mut held, &mut ends, give);
        }

        // A large value: its objects, forwards from the first.
        let start = match &range.0 {
            Bound::Included(start) | Bound::Excluded(start) if *start > value => range.0.clone(),
            _ => Bound::Included(value.clone()),
        };

        source
            .range_in(tree, as_ref(&start), as_ref(&range.1), false)?
            .for_each(&mut |entry, stored| {
                let (entry, key) = entry_parts(source, index, entry, stored)?;

                if entry != value.as_slice() {
                    return Ok(true);
                }

                stopped = give(key)?;

                Ok(stopped)
            })?;

        if stopped {
            return Ok(true);
        }

        // Every entry of the value is greater than the value alone.
        high = Bound::Excluded(value.clone());
    }
}

/// Gives the keys held back, the last first, until `give` says to stop, and
/// returns whether it did. Nothing is held afterwards.
fn give_held(
    held: &mut Vec<u8>,
    ends: &mut Vec<usize>,
    give: &mut dyn FnMut(&[u8]) -> Result<bool>,
) -> Result<bool> {
    while let Some(end) = ends.pop() {
        let start = ends.last().copied().unwrap_or(0);
        let stop = give(&held[start..end])?;

        held.truncate(start);

        if stop {
            ends.clear();
            held.clear();

            return Ok(true);
        }
    }

    Ok(false)
}

fn as_ref(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(bytes) => Bound::Included(bytes),
        Bound::Excluded(bytes) => Bound::Excluded(bytes),
        Bound::Unbounded => Bound::Unbounded,
    }
}

/// An object found: its primary key, its record, and the values it sorts
/// by.
struct Found {
    sort: Vec<Value>,
    key: Vec<u8>,
    record: Vec<u8>,
}

/// Runs `plan` and returns the objects it finds, in its order. Only the
/// objects in the result are decoded whole.
pub(crate) fn objects(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Object>> {
    let mut objects = Vec::new();

    found(source, plan, &mut |record| {
        objects.push(objects::decode(source, plan.collection, &record)?);

        Ok(())
    })?;

    Ok(objects)
}

/// Runs `plan` and returns the records of the objects it finds, in its
/// order, as the collection's tree holds them.
pub(crate) fn stored(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Vec<u8>>> {
    let mut records = Vec::new();

    found(source, plan, &mut |record| {
        records.push(record.into_owned());

        Ok(())
    })?;

    Ok(records)
}

/// Runs `plan` and gives `visit` the record of each object it finds, in its
/// order, borrowed: where the tree holds it, for an object the walk delivers
/// in the query's order.
pub(crate) fn each_stored(
    source: &dyn Source,
    plan: &Plan<'_>,
    visit: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    found(source, plan, &mut |record| visit(&record))
}

/// What [`found`] gives the record of each object in the result to, in the
/// result's order: borrowed from the walk, or owned when it was read
/// alone.
type Take<'t> = dyn FnMut(Cow<'_, [u8]>) -> Result<()> + 't;

#[cfg(test)]
thread_local! {
    /// Whether the filter and the sort read whole objects, as the plain
    /// answer the tests compare every plan with does.
    static WHOLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `run` with the filter and the sort reading objects decoded whole,
/// rather than fields found in records, so that the two ways are compared.
#[cfg(test)]
pub(crate) fn whole<T>(run: impl FnOnce() -> T) -> T {
    WHOLE.with(|whole| whole.set(true));

    let result = run();

    WHOLE.with(|whole| whole.set(false));
    result
}

/// Gives `take` the record of each object `plan` finds, in its order.
///
/// When the walk delivers the objects in the query's order, each is given
/// as the walk finds it, with the record the walk lent or the one read for
/// it, and nothing of it is copied. Otherwise the objects are gathered and
/// sorted first.
fn found(source: &dyn Source, plan: &Plan<'_>, take: &mut Take<'_>) -> Result<()> {
    let reader = Reader {
        source,
        links: RefCell::new(HashMap::new()),
    };
    let limit = plan.limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit).unwrap_or(usize::MAX)
    });
    let offset = usize::try_from(plan.offset).unwrap_or(usize::MAX);

    if limit == 0 {
        return Ok(());
    }

    if plan.ordered {
        let mut skipped = 0;
        let mut taken = 0;

        walk(source, plan, &mut |key, walked| {
            let mut stop = false;

            with_record(source, plan, key, walked, &mut |record| {
                if meets(&reader, plan, Some(record), false)?.is_none() {
                    return Ok(());
                }

                if skipped < offset {
                    skipped += 1;

                    return Ok(());
                }

                take(Cow::Borrowed(record))?;
                taken += 1;
                stop = taken >= limit;

                Ok(())
            })?;

            Ok(stop)
        })?;

        return Ok(());
    }

    let keep = offset.saturating_add(limit);
    let compare = |a: &Found, b: &Found| order(plan, a, b);
    let mut found: Vec<Found> = Vec::new();
    // Once the kept objects have been cut down to `keep`, the last of them:
    // an object that sorts after it cannot be in the result, and is not
    // copied.
    let mut trimmed = false;
    // Without a sort the result is in primary key order, so a key alone says
    // whether its object can still be in the result, and one that cannot is
    // not read. When the index answers the filter too, its entries decide
    // which objects are found, and only the records of the result are read.
    let by_key = plan.sort.is_empty();
    let deferred = by_key && plan.filter.is_none() && matches!(plan.access, Access::Index { .. });

    walk(source, plan, &mut |key, record| {
        let bound = trimmed.then(|| &found[keep - 1]);

        if by_key && bound.is_some_and(|bound| key >= bound.key.as_slice()) {
            return Ok(false);
        }

        let hit = if deferred {
            Found {
                sort: Vec::new(),
                key: key.to_vec(),
                record: Vec::new(),
            }
        } else {
            let admit = |values: &[Value], key: &[u8]| {
                bound.is_none_or(|bound| {
                    order_of(plan, (values, key), (&bound.sort, &bound.key)).is_lt()
                })
            };
            let Some(hit) = read(&reader, plan, key, record, &admit)? else {
                return Ok(false);
            };

            hit
        };

        found.push(hit);

        // Only the first `offset + limit` can be in the result, so the rest
        // are dropped as they fall behind.
        if keep < usize::MAX && found.len() >= keep.saturating_mul(2).max(TRIM_AT) {
            found.select_nth_unstable_by(keep - 1, compare);
            found.truncate(keep);
            trimmed = true;
        }

        Ok(false)
    })?;

    found.sort_by(compare);

    for hit in found.into_iter().skip(offset).take(limit) {
        if deferred {
            with_record(source, plan, &hit.key, None, &mut |record| {
                take(Cow::Borrowed(record))
            })?;
        } else {
            take(Cow::Owned(hit.record))?;
        }
    }

    Ok(())
}

/// The order of the result: the sort keys, then the primary key.
fn order(plan: &Plan<'_>, a: &Found, b: &Found) -> Ordering {
    order_of(plan, (&a.sort, &a.key), (&b.sort, &b.key))
}

/// [`order`] of two objects given by the values they sort by and their keys.
fn order_of(plan: &Plan<'_>, a: (&[Value], &[u8]), b: (&[Value], &[u8])) -> Ordering {
    for (position, (_, descending)) in plan.sort.iter().enumerate() {
        let order = key::compare(&a.0[position], &b.0[position]);
        let order = if *descending { order.reverse() } else { order };

        if order.is_ne() {
            return order;
        }
    }

    a.1.cmp(b.1)
}

/// Whether an object with the values it sorts by and its key may be in a
/// result.
type Admit<'a> = dyn Fn(&[Value], &[u8]) -> bool + 'a;

/// The object with primary key `key`, whose record the walk may have lent
/// already, if it meets the plan's filter, with the values it sorts by, and
/// only if `admit` says an object with those values and that key may be in
/// the result. The filter and the sort read the fields they need from the
/// record; the key and the record are copied only for an object that is
/// kept.
fn read(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    key: &[u8],
    walked: Option<&[u8]>,
    admit: &Admit<'_>,
) -> Result<Option<Found>> {
    let mut hit = None;

    with_record(reader.source, plan, key, walked, &mut |record| {
        if let Some(values) = meets(reader, plan, Some(record), true)? {
            if admit(&values, key) {
                hit = Some(Found {
                    sort: values,
                    key: key.to_vec(),
                    record: record.to_vec(),
                });
            }
        }

        Ok(())
    })?;

    Ok(hit)
}

/// Gives `visit` the record of the object with primary key `key`: the one
/// the walk lent, or else the object's record, read where it lies. Either
/// way it is borrowed, so that a record the query does not keep is never
/// copied.
fn with_record(
    source: &dyn Source,
    plan: &Plan<'_>,
    key: &[u8],
    walked: Option<&[u8]>,
    visit: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    if let Some(record) = walked {
        return visit(record);
    }

    if source.get_in_with(&records(plan.collection.id), key, visit)? {
        Ok(())
    } else {
        Err(source.corrupted(format!(
            "an index of `{}` names an object that is not there",
            plan.collection.name
        )))
    }
}

/// Whether the object whose record is `record` meets the plan's filter, with
/// the values it sorts by when `sort` is set: `None` when it does not.
fn meets(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    record: Option<&[u8]>,
    sort: bool,
) -> Result<Option<Vec<Value>>> {
    let record = record.unwrap_or_default();
    let sorting = sort && !plan.sort.is_empty();

    if plan.filter.is_none() && !sorting {
        return Ok(Some(Vec::new()));
    }

    let view = View {
        source: reader.source,
        collection: plan.collection,
        record,
    };
    #[cfg(test)]
    let object = WHOLE
        .with(std::cell::Cell::get)
        .then(|| objects::decode(reader.source, plan.collection, record))
        .transpose()?;
    #[cfg(test)]
    let fields: &dyn Fields = match &object {
        Some(object) => object,
        None => &view,
    };
    #[cfg(not(test))]
    let fields: &dyn Fields = &view;

    if let Some(filter) = &plan.filter {
        if !reader.holds(filter, fields)? {
            return Ok(None);
        }
    }

    if !sorting {
        return Ok(Some(Vec::new()));
    }

    let values: Vec<Value> = plan
        .sort
        .iter()
        .map(|(path, _)| reader.value_at(fields, path))
        .collect::<Result<_>>()?;

    Ok(Some(values))
}

/// Runs `plan` and counts the objects it finds, after its offset and within
/// its limit.
pub(crate) fn count(source: &dyn Source, plan: &Plan<'_>) -> Result<u64> {
    let found = if plan.filter.is_none() && !plan.exact {
        // No filter at all: the tree knows how many records it holds.
        source.len_in(&records(plan.collection.id))?
    } else if plan.exact {
        match &plan.access {
            // Each object has one entry in the ranges, which do not overlap,
            // so the count is the entries', which the tree gives a leaf at a
            // time.
            Access::Index {
                index,
                ranges,
                repeats: false,
                ..
            } => {
                let tree = index_tree(index.id);
                let mut found = 0u64;

                for range in ranges {
                    found += source
                        .range_in(&tree, as_ref(&range.0), as_ref(&range.1), false)?
                        .count_entries()?;
                }

                found
            }
            Access::Records { range, .. } => source
                .range_in(
                    &records(plan.collection.id),
                    as_ref(&range.0),
                    as_ref(&range.1),
                    false,
                )?
                .count_entries()?,
            _ => {
                let mut found = 0u64;

                walk(source, plan, &mut |_, _| {
                    found += 1;

                    Ok(false)
                })?;

                found
            }
        }
    } else {
        let reader = Reader {
            source,
            links: RefCell::new(HashMap::new()),
        };
        let mut found = 0u64;

        walk(source, plan, &mut |key, walked| {
            with_record(source, plan, key, walked, &mut |record| {
                found += u64::from(meets(&reader, plan, Some(record), false)?.is_some());

                Ok(())
            })?;

            Ok(false)
        })?;

        found
    };
    let found = found.saturating_sub(plan.offset);

    Ok(plan.limit.map_or(found, |limit| found.min(limit)))
}
