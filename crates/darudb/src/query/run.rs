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
use crate::format::object::schema::CollectionDef;
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

            for range in ordered {
                // Walking backwards, the objects of one value come in
                // descending key order; they are held back and given in
                // ascending key order, which is the order for ties.
                let mut group: Vec<Vec<u8>> = Vec::new();
                let mut group_value: Vec<u8> = Vec::new();
                let mut stop = false;
                let mut give = |key: Vec<u8>, seen: &mut HashSet<Vec<u8>>| -> Result<bool> {
                    if *repeats && !seen.insert(key.clone()) {
                        return Ok(false);
                    }

                    visit(key, None)
                };

                for entry in
                    source.range_in(&tree, as_ref(&range.0), as_ref(&range.1), *backward)?
                {
                    let (entry, value) = entry?;
                    let (_, used) = key::decode(&entry).map_err(|reason| {
                        source.corrupted(format!("an entry of index {}: {reason}", index.id))
                    })?;
                    let key = if index.unique {
                        value
                    } else {
                        entry[used..].to_vec()
                    };

                    if !*backward {
                        if give(key, &mut seen)? {
                            stop = true;
                            break;
                        }

                        continue;
                    }

                    if entry[..used] != group_value[..] {
                        while let Some(key) = group.pop() {
                            if give(key, &mut seen)? {
                                stop = true;
                                break;
                            }
                        }

                        if stop {
                            break;
                        }

                        group_value = entry[..used].to_vec();
                    }

                    group.push(key);
                }

                while !stop {
                    let Some(key) = group.pop() else {
                        break;
                    };

                    stop = give(key, &mut seen)?;
                }

                if stop {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn as_ref(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(bytes) => Bound::Included(bytes),
        Bound::Excluded(bytes) => Bound::Excluded(bytes),
        Bound::Unbounded => Bound::Unbounded,
    }
}

/// An object found: its primary key, its record, the object when it was
/// decoded, and the values it sorts by.
struct Found {
    sort: Vec<Value>,
    key: Vec<u8>,
    record: Vec<u8>,
    object: Option<Object>,
}

/// Runs `plan` and returns the objects it finds, in its order.
pub(crate) fn objects(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Object>> {
    found(source, plan, true)?
        .into_iter()
        .map(|found| {
            found
                .object
                .ok_or_else(|| objects::internal("an object found was not decoded"))
        })
        .collect()
}

/// Runs `plan` and returns the records of the objects it finds, in its
/// order, as the collection's tree holds them. An object is decoded only when
/// the filter or the sort has to read it.
pub(crate) fn stored(source: &dyn Source, plan: &Plan<'_>) -> Result<Vec<Vec<u8>>> {
    Ok(found(source, plan, false)?
        .into_iter()
        .map(|found| found.record)
        .collect())
}

/// What `plan` finds, in its order, with the objects decoded if `decode` is
/// set.
fn found(source: &dyn Source, plan: &Plan<'_>, decode: bool) -> Result<Vec<Found>> {
    let reader = Reader {
        source,
        links: RefCell::new(HashMap::new()),
    };
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
            let Some(hit) = read(&reader, plan, key, record, decode)? else {
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
        let Some(mut hit) = read(&reader, plan, key, record, decode)? else {
            return Ok(false);
        };

        if let Some(object) = &hit.object {
            hit.sort = plan
                .sort
                .iter()
                .map(|(path, _)| reader.value_at(object, path))
                .collect::<Result<_>>()?;
        }

        if !decode {
            hit.object = None;
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
/// already, if it meets the plan's filter. The object is decoded if `decode`
/// is set, or if the filter or the sort needs it.
fn read(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    key: Vec<u8>,
    record: Option<Vec<u8>>,
    decode: bool,
) -> Result<Option<Found>> {
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
    let object = if decode || plan.filter.is_some() || !plan.sort.is_empty() {
        Some(objects::decode(reader.source, plan.collection, &record)?)
    } else {
        None
    };

    if let (Some(filter), Some(object)) = (&plan.filter, &object) {
        if !reader.holds(filter, object)? {
            return Ok(None);
        }
    }

    Ok(Some(Found {
        sort: Vec::new(),
        key,
        record,
        object,
    }))
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
        let mut found = 0u64;

        walk(source, plan, &mut |key, record| {
            if read(&reader, plan, key, record, false)?.is_some() {
                found += 1;
            }

            Ok(false)
        })?;

        found
    };
    let found = found.saturating_sub(plan.offset);

    Ok(plan.limit.map_or(found, |limit| found.min(limit)))
}
