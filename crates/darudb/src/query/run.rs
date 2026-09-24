//! Running a planned query: walking its access, testing each object read,
//! sorting, and skipping and stopping (`design/objects.md`, "Running a
//! query").

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::Bound;

use super::ir::Op;
use super::plan::{Access, Cond, Plan, Range, Resolved, Step, Test};
use crate::error::Result;
use crate::format::object::schema::{CollectionDef, IndexDef};
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
        test: &mut dyn FnMut(&Value) -> bool,
    ) -> Result<bool> {
        let Some((step, rest)) = steps.split_first() else {
            return Ok(test(value));
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

    /// Whether `test` holds for any value at `path` of `object`.
    fn any_in(
        &self,
        object: &Object,
        path: &Resolved<'_>,
        test: &mut dyn FnMut(&Value) -> bool,
    ) -> Result<bool> {
        match path.steps.split_first() {
            Some((Step::Field(name), rest)) => {
                self.any(object.get(name).unwrap_or(&Value::Null), rest, test)
            }
            _ => Ok(false),
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

    fn holds(&self, cond: &Cond<'_>, object: &Object) -> Result<bool> {
        match cond {
            Cond::And(terms) => {
                for term in terms {
                    if !self.holds(term, object)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            Cond::Or(terms) => {
                for term in terms {
                    if self.holds(term, object)? {
                        return Ok(true);
                    }
                }

                Ok(false)
            }
            Cond::Not(term) => Ok(!self.holds(term, object)?),
            Cond::Test {
                path,
                test: Test::IsNull,
            } => self.any_in(object, path, &mut Value::is_null),
            Cond::Test { path, test } => self.any_in(object, path, &mut |value| {
                !value.is_null() && passes(test, value)
            }),
        }
    }

    /// The value at a single-valued `path` of `object`, for sorting.
    fn value_at(&self, object: &Object, path: &Resolved<'_>) -> Result<Value> {
        let mut found = Value::Null;

        self.any_in(object, path, &mut |value| {
            found = value.clone();
            true
        })?;

        Ok(found)
    }
}

/// Whether `test` holds for the value `value`, which is not null.
fn passes(test: &Test, value: &Value) -> bool {
    match test {
        Test::Compare(op, other) => {
            let order = key::compare(value, other);

            match op {
                Op::Eq => order.is_eq(),
                Op::Ne => order.is_ne(),
                Op::Lt => order.is_lt(),
                Op::Le => order.is_le(),
                Op::Gt => order.is_gt(),
                _ => order.is_ge(),
            }
        }
        Test::Between(low, high) => {
            key::compare(value, low).is_ge() && key::compare(value, high).is_le()
        }
        Test::In(values) => values
            .binary_search_by(|other| key::compare(other, value))
            .is_ok(),
        Test::Element(other) => key::compare(value, other).is_eq(),
        Test::Substring(text) => value
            .as_str()
            .is_some_and(|value| value.contains(text.as_str())),
        Test::StartsWith(text) => value
            .as_str()
            .is_some_and(|value| value.starts_with(text.as_str())),
        Test::EndsWith(text) => value
            .as_str()
            .is_some_and(|value| value.ends_with(text.as_str())),
        Test::IsNull => value.is_null(),
    }
}

/// Walks the keys the access names, in its order, reading each object once.
/// `visit` gets a primary key and, when the walk read it, the record; it
/// returns whether to stop.
fn walk(
    source: &dyn Source,
    plan: &Plan<'_>,
    visit: &mut dyn FnMut(Vec<u8>, Option<Vec<u8>>) -> Result<bool>,
) -> Result<()> {
    let records = records(plan.collection.id);

    match &plan.access {
        Access::Records { range, backward } => {
            for entry in source.range_in(&records, as_ref(&range.0), as_ref(&range.1), *backward)? {
                let (key, record) = entry?;

                if visit(key, Some(record))? {
                    break;
                }
            }
        }
        Access::Keys { keys, backward } => {
            let mut ordered: Vec<&Vec<u8>> = keys.iter().collect();

            if *backward {
                ordered.reverse();
            }

            for key in ordered {
                if let Some(record) = source.get_in(&records, key)? {
                    if visit(key.clone(), Some(record))? {
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
        } => {
            let tree = index_tree(index.id);
            let mut seen = HashSet::new();
            let mut ordered: Vec<&Range> = ranges.iter().collect();

            if *backward {
                ordered.reverse();
            }

            let mut give = |key: Vec<u8>, seen: &mut HashSet<Vec<u8>>| -> Result<bool> {
                if *repeats && !seen.insert(key.clone()) {
                    return Ok(false);
                }

                visit(key, None)
            };

            for range in ordered {
                let stop = if *backward {
                    walk_back(source, index, &tree, range, &mut |key| give(key, &mut seen))?
                } else {
                    walk_entries(source, index, &tree, range, *backward, &mut |key| {
                        give(key, &mut seen)
                    })?
                };

                if stop {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// How many objects of one value a backward walk holds back before it goes
/// to the value's first entry and walks its objects forwards instead.
const GROUP: usize = 64;

/// The primary key an index entry names, and how many bytes of the entry
/// its value takes.
fn entry_key(
    source: &dyn Source,
    index: &IndexDef,
    entry: Vec<u8>,
    value: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let (_, used) = key::decode(&entry)
        .map_err(|reason| source.corrupted(format!("an entry of index {}: {reason}", index.id)))?;
    let mut entry = entry;
    let key = entry.split_off(used);

    // A unique index keeps the key in the entry's value, and after the
    // value only where values repeat: null.
    Ok((entry, if index.unique { value } else { key }))
}

/// Gives the primary keys of the entries of `range`, in the walk's order,
/// until `give` says to stop, and returns whether it did.
fn walk_entries(
    source: &dyn Source,
    index: &IndexDef,
    tree: &str,
    range: &Range,
    backward: bool,
    give: &mut dyn FnMut(Vec<u8>) -> Result<bool>,
) -> Result<bool> {
    for entry in source.range_in(tree, as_ref(&range.0), as_ref(&range.1), backward)? {
        let (entry, value) = entry?;
        let (_, key) = entry_key(source, index, entry, value)?;

        if give(key)? {
            return Ok(true);
        }
    }

    Ok(false)
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
    give: &mut dyn FnMut(Vec<u8>) -> Result<bool>,
) -> Result<bool> {
    let mut high = range.1.clone();

    'values: loop {
        let mut group: Vec<Vec<u8>> = Vec::new();
        let mut value: Vec<u8> = Vec::new();

        for entry in source.range_in(tree, as_ref(&range.0), as_ref(&high), true)? {
            let (entry, key) = {
                let (entry, stored) = entry?;

                entry_key(source, index, entry, stored)?
            };

            if entry != value {
                while let Some(key) = group.pop() {
                    if give(key)? {
                        return Ok(true);
                    }
                }

                value = entry;
            }

            group.push(key);

            if group.len() < GROUP {
                continue;
            }

            // A large value: its objects, forwards from the first.
            let start = match &range.0 {
                Bound::Included(start) | Bound::Excluded(start) if *start > value => {
                    range.0.clone()
                }
                _ => Bound::Included(value.clone()),
            };

            for entry in source.range_in(tree, as_ref(&start), as_ref(&range.1), false)? {
                let (entry, stored) = entry?;
                let (entry, key) = entry_key(source, index, entry, stored)?;

                if entry != value {
                    break;
                }

                if give(key)? {
                    return Ok(true);
                }
            }

            // Every entry of the value is greater than the value alone.
            high = Bound::Excluded(value);

            continue 'values;
        }

        while let Some(key) = group.pop() {
            if give(key)? {
                return Ok(true);
            }
        }

        return Ok(false);
    }
}

fn as_ref(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(bytes) => Bound::Included(bytes),
        Bound::Excluded(bytes) => Bound::Excluded(bytes),
        Bound::Unbounded => Bound::Unbounded,
    }
}

/// An object found: its primary key, its record, the fields the filter and
/// the sort read, and the values it sorts by.
struct Found {
    sort: Vec<Value>,
    key: Vec<u8>,
    record: Vec<u8>,
}

/// Runs `plan` and returns the objects it finds, in its order. Only the
/// objects in the result are decoded whole.
pub(crate) fn objects(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Object>> {
    found(source, plan)?
        .into_iter()
        .map(|found| objects::decode(source, plan.collection, &found.record))
        .collect()
}

/// Runs `plan` and returns the records of the objects it finds, in its
/// order, as the collection's tree holds them.
pub(crate) fn stored(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Vec<u8>>> {
    Ok(found(source, plan)?
        .into_iter()
        .map(|found| found.record)
        .collect())
}

/// The ids of the fields of the collection itself that the filter and the
/// sort of `plan` read, sorted: all an object has to be decoded into for
/// them.
fn wanted(plan: &Plan<'_>) -> Vec<u64> {
    fn add(fields: &mut Vec<u64>, plan: &Plan<'_>, path: &Resolved<'_>) {
        if let Some(Step::Field(name)) = path.steps.first() {
            if let Some(field) = plan.collection.fields.by_name(name) {
                fields.push(field.id);
            }
        }
    }

    fn walk_cond(fields: &mut Vec<u64>, plan: &Plan<'_>, cond: &Cond<'_>) {
        match cond {
            Cond::And(terms) | Cond::Or(terms) => {
                for term in terms {
                    walk_cond(fields, plan, term);
                }
            }
            Cond::Not(term) => walk_cond(fields, plan, term),
            Cond::Test { path, .. } => add(fields, plan, path),
        }
    }

    let mut fields = Vec::new();

    #[cfg(test)]
    if WHOLE.with(std::cell::Cell::get) {
        fields.extend(plan.collection.fields.list.iter().map(|field| field.id));
    }

    if let Some(filter) = &plan.filter {
        walk_cond(&mut fields, plan, filter);
    }

    for (path, _) in &plan.sort {
        add(&mut fields, plan, path);
    }

    fields.sort_unstable();
    fields.dedup();
    fields
}

#[cfg(test)]
thread_local! {
    /// Whether the filter and the sort read whole objects, as the plain
    /// answer the tests compare every plan with does.
    static WHOLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `run` with the filter and the sort reading whole objects, so that a
/// field left out of [`wanted`] shows as a difference from the plan.
#[cfg(test)]
pub(crate) fn whole<T>(run: impl FnOnce() -> T) -> T {
    WHOLE.with(|whole| whole.set(true));

    let result = run();

    WHOLE.with(|whole| whole.set(false));
    result
}

/// What `plan` finds, in its order.
fn found(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Found>> {
    let reader = Reader {
        source,
        links: RefCell::new(HashMap::new()),
    };
    let wanted = wanted(plan);
    let limit = plan.limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit).unwrap_or(usize::MAX)
    });
    let offset = usize::try_from(plan.offset).unwrap_or(usize::MAX);

    if limit == 0 {
        return Ok(Vec::new());
    }

    if plan.ordered {
        let mut skipped = 0;
        let mut found = Vec::new();

        walk(source, plan, &mut |key, record| {
            let Some((hit, _)) = read(&reader, plan, &wanted, key, record)? else {
                return Ok(false);
            };

            if skipped < offset {
                skipped += 1;

                return Ok(false);
            }

            found.push(hit);

            Ok(found.len() >= limit)
        })?;

        return Ok(found);
    }

    let keep = offset.saturating_add(limit);
    let compare = |a: &Found, b: &Found| order(plan, a, b);
    let mut found: Vec<Found> = Vec::new();

    walk(source, plan, &mut |key, record| {
        let Some((mut hit, fields)) = read(&reader, plan, &wanted, key, record)? else {
            return Ok(false);
        };

        if let Some(fields) = &fields {
            hit.sort = plan
                .sort
                .iter()
                .map(|(path, _)| reader.value_at(fields, path))
                .collect::<Result<_>>()?;
        }

        found.push(hit);

        // Only the first `offset + limit` can be in the result, so the rest
        // are dropped as they fall behind.
        if keep < usize::MAX && found.len() >= keep.saturating_mul(2).max(TRIM_AT) {
            found.select_nth_unstable_by(keep - 1, compare);
            found.truncate(keep);
        }

        Ok(false)
    })?;

    found.sort_by(compare);

    Ok(found.into_iter().skip(offset).take(limit).collect())
}

/// The order of the result: the sort keys, then the primary key.
fn order(plan: &Plan<'_>, a: &Found, b: &Found) -> Ordering {
    for (position, (_, descending)) in plan.sort.iter().enumerate() {
        let order = key::compare(&a.sort[position], &b.sort[position]);
        let order = if *descending { order.reverse() } else { order };

        if order.is_ne() {
            return order;
        }
    }

    a.key.cmp(&b.key)
}

/// The object with primary key `key`, whose record the walk may have read
/// already, if it meets the plan's filter, with the fields `wanted` that the
/// filter and the sort read, decoded when there are any.
fn read(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    wanted: &[u64],
    key: Vec<u8>,
    record: Option<Vec<u8>>,
) -> Result<Option<(Found, Option<Object>)>> {
    let record = match record {
        Some(record) => record,
        None => reader
            .source
            .get_in(&records(plan.collection.id), &key)?
            .ok_or_else(|| {
                reader.source.corrupted(format!(
                    "an index of `{}` names an object that is not there",
                    plan.collection.name
                ))
            })?,
    };
    let fields = if plan.filter.is_some() || !plan.sort.is_empty() {
        Some(objects::decode_some(
            reader.source,
            plan.collection,
            &record,
            wanted,
        )?)
    } else {
        None
    };

    if let (Some(filter), Some(fields)) = (&plan.filter, &fields) {
        if !reader.holds(filter, fields)? {
            return Ok(None);
        }
    }

    Ok(Some((
        Found {
            sort: Vec::new(),
            key,
            record,
        },
        fields,
    )))
}

/// Runs `plan` and counts the objects it finds, after its offset and within
/// its limit.
pub(crate) fn count(source: &dyn Source, plan: &Plan<'_>) -> Result<u64> {
    let found = if plan.filter.is_none() && !plan.exact {
        // No filter at all: the tree knows how many records it holds.
        source.len_in(&records(plan.collection.id))?
    } else if plan.exact {
        let mut found = 0u64;

        walk(source, plan, &mut |_, _| {
            found += 1;

            Ok(false)
        })?;

        found
    } else {
        let reader = Reader {
            source,
            links: RefCell::new(HashMap::new()),
        };
        let wanted = wanted(plan);
        let mut found = 0u64;

        walk(source, plan, &mut |key, record| {
            if read(&reader, plan, &wanted, key, record)?.is_some() {
                found += 1;
            }

            Ok(false)
        })?;

        found
    };
    let found = found.saturating_sub(plan.offset);

    Ok(plan.limit.map_or(found, |limit| found.min(limit)))
}
