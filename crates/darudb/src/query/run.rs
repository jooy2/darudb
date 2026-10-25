//! Running a planned query: walking its access, testing each object read,
//! sorting, and skipping and stopping (`design/objects.md`, "Running a
//! query").

use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::Bound;

use super::ir::{Expr, Ir, Op, Operand};
use super::keys::KeySet;
use super::plan::{Access, Cond, Part, Plan, Range, Resolved, Step, Test, terms_of};
use crate::btree::UNMARKED;
use crate::error::{Error, Result};
use crate::format::object::codec::{self, FieldRef, NameOrder};
use crate::format::object::names::{index_tree, records};
use crate::format::object::schema::{CollectionDef, FieldDef, IndexDef, Kind};
use crate::format::object::{Object, Value, key};
use crate::schema::objects::{self, Source};
use crate::txn::Seeker;

/// How many linked objects a query keeps once read, so that a filter or a
/// sort that follows the same link twice reads the object once.
const LINK_CACHE: usize = 4096;

/// Below this many objects kept for a sort with a limit, the kept objects
/// are not trimmed to the limit.
const TRIM_AT: usize = 256;

/// Linked objects already read, by collection id and encoded key.
type Links = HashMap<(u64, Vec<u8>), Option<Object>>;

/// A query's reads: its transaction, the linked objects already read, and
/// the lookups of the records an index names.
struct Reader<'a> {
    source: &'a dyn Source,
    links: RefCell<Links>,
    records: RefCell<Records<'a>>,
    /// The terms of the plan's filter, a bit for each by its position, that
    /// a set of keys answered already, and that are not tested again.
    skip: u64,
    /// Whether the set answered every term, so that nothing is tested.
    answered: bool,
}

/// How a query reads the records an index names: the first one from the
/// root, as a query of one object reads it, which is not worth the seeker's
/// path, and the rest through a seeker made for the second.
enum Records<'a> {
    None,
    One,
    Many(Seeker<'a>),
}

impl<'a> Reader<'a> {
    fn new(source: &'a dyn Source) -> Self {
        Self {
            source,
            links: RefCell::new(HashMap::new()),
            records: RefCell::new(Records::None),
            skip: 0,
            answered: false,
        }
    }

    /// A reader for a run of `plan` that leaves out the terms `skip` marks.
    fn skipping(source: &'a dyn Source, plan: &Plan<'_>, skip: u64) -> Self {
        Self {
            skip,
            answered: plan
                .filter
                .as_ref()
                .is_some_and(|filter| answered(filter, skip)),
            ..Self::new(source)
        }
    }
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
    fn any_in<F: Fields + ?Sized>(
        &self,
        fields: &F,
        path: &Resolved<'_>,
        test: &mut dyn FnMut(ValueRef<'_>) -> bool,
    ) -> Result<bool> {
        let Some((Step::Field(name), rest)) = path.steps.split_first() else {
            return Ok(false);
        };
        // A field of the collection itself was found in the schema when the
        // query was planned, and is not looked up by name for each object.
        let current = match path.direct {
            Some(field) => fields.field_of(field)?,
            None => fields.field(name)?,
        };

        match current {
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

        let object = objects::get(self.source, target, &records(target.id), None, key)?;
        let mut links = self.links.borrow_mut();

        if links.len() >= LINK_CACHE {
            links.clear();
        }

        links.insert(cache_key, object.clone());

        Ok(object)
    }

    /// [`holds`](Self::holds) of a plan's filter, leaving out the terms a
    /// set of keys answered.
    fn holds_left<F: Fields + ?Sized>(&self, filter: &Cond<'_>, fields: &F) -> Result<bool> {
        match filter {
            Cond::And(terms) if self.skip != 0 => {
                for (position, term) in terms.iter().enumerate() {
                    if !skipped(self.skip, position) && !self.holds(term, fields)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            _ => self.holds(filter, fields),
        }
    }

    fn holds<F: Fields + ?Sized>(&self, cond: &Cond<'_>, fields: &F) -> Result<bool> {
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
    fn value_at<F: Fields + ?Sized>(&self, fields: &F, path: &Resolved<'_>) -> Result<Value> {
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
    /// A string's bytes, checked to be UTF-8 already, which a comparison
    /// reads as they are.
    String(&'a [u8]),
    Bytes(&'a [u8]),
}

impl ValueRef<'_> {
    fn is_null(self) -> bool {
        matches!(self, ValueRef::Null | ValueRef::Value(Value::Null))
    }

    /// The bytes of a string value.
    fn string_bytes(&self) -> Option<&[u8]> {
        match self {
            ValueRef::String(text) => Some(text),
            ValueRef::Value(Value::String(text)) => Some(text.as_bytes()),
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
            ValueRef::String(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
            ValueRef::Bytes(value) => Value::Bytes(value.to_vec()),
        }
    }

    /// Whether this value equals `other`, as [`compare`](Self::compare)
    /// finds them equal: strings and bytes compared as bytes, which tells two
    /// of different lengths apart without reading them, as nearly every
    /// object an equality on a field without an index tests differs.
    #[inline(always)]
    fn equals(self, other: &Value) -> bool {
        match (self, other) {
            (ValueRef::String(value), Value::String(other)) => same_bytes(value, other.as_bytes()),
            (ValueRef::Bytes(value), Value::Bytes(other)) => same_bytes(value, other),
            _ => self.compare(other).is_eq(),
        }
    }

    /// The order of this value and `other`, as [`key::compare`] gives it.
    fn compare(self, other: &Value) -> Ordering {
        match (self, other) {
            (ValueRef::Value(value), _) => key::compare(value, other),
            // A number compared with a number of its own type, as a sort or
            // a range on a numeric field compares every object it reads,
            // without a value made for it to go through `key::compare`.
            (ValueRef::Int(value), Value::Int(other)) => value.cmp(other),
            (ValueRef::Float(value), Value::Float(other)) => key::compare_floats(value, *other),
            (ValueRef::String(value), Value::String(other)) => value.cmp(other.as_bytes()),
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

/// Whether two byte strings are the same, compared a word at a time in
/// place when they are no longer than sixteen bytes: a call to compare
/// memory for each object an equality on a short string tests cost more
/// than the comparison.
#[inline(always)]
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    let len = a.len();

    if len != b.len() {
        return false;
    }

    // Two words that overlap where the length is not a multiple of one.
    match len {
        0..=3 => a == b,
        4..=8 => word4(a, 0) == word4(b, 0) && word4(a, len - 4) == word4(b, len - 4),
        9..=16 => word8(a, 0) == word8(b, 0) && word8(a, len - 8) == word8(b, len - 8),
        _ => a == b,
    }
}

/// The four bytes of `bytes` from `at`, as a number.
#[inline(always)]
fn word4(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The eight bytes of `bytes` from `at`, as a number.
#[inline(always)]
fn word8(bytes: &[u8], at: usize) -> u64 {
    let mut word = [0u8; 8];

    word.copy_from_slice(&bytes[at..at + 8]);
    u64::from_ne_bytes(word)
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

    /// The value of `field`, one of the collection's fields.
    fn field_of<'a>(&'a self, field: &'a FieldDef) -> Result<Current<'a>>;
}

impl Fields for Object {
    fn field(&self, name: &str) -> Result<Current<'_>> {
        Ok(Current::Borrowed(
            self.get(name).map_or(ValueRef::Null, ValueRef::Value),
        ))
    }

    fn field_of<'a>(&'a self, field: &'a FieldDef) -> Result<Current<'a>> {
        self.field(&field.name)
    }
}

/// An object's record, whose fields are found in it one at a time: nothing
/// is decoded but the fields read, and nothing copied but those of them that
/// are not scalars.
struct View<'a> {
    source: &'a dyn Source,
    collection: &'a CollectionDef,
    record: &'a [u8],
    /// The field the walk marked the record for, and its mark.
    marked: Option<u64>,
    mark: u16,
}

impl<'a> View<'a> {
    fn new(reader: &'a Reader<'_>, plan: &'a Plan<'_>, lent: Lent<'a>) -> Self {
        Self {
            source: reader.source,
            collection: plan.collection,
            record: lent.record,
            marked: plan.marked.map(|field| field.id),
            mark: lent.mark,
        }
    }
}

/// A record and its mark, [`UNMARKED`] unless a walk over every record lent
/// it marked for the plan's [`marked`](Plan::marked) field: 0 where the
/// record leaves the field out, and otherwise where the field's value
/// starts.
#[derive(Clone, Copy)]
struct Lent<'r> {
    record: &'r [u8],
    mark: u16,
}

impl Default for Lent<'_> {
    fn default() -> Self {
        Self::unmarked(&[])
    }
}

impl<'r> Lent<'r> {
    fn unmarked(record: &'r [u8]) -> Self {
        Self {
            record,
            mark: UNMARKED,
        }
    }
}

/// What a record holds at a mark that tells the record leaves the field out.
const ABSENT: u16 = 0;

/// The mark of `record` for field `id`, which a walk keeps with the leaf the
/// record is in: a record that does not read is left unmarked, for the
/// query to find its damage when it reads the record.
fn field_mark(record: &[u8], id: u64) -> u16 {
    match codec::locate_field(record, id) {
        Ok(Some(at)) => u16::try_from(at).unwrap_or(UNMARKED),
        Ok(None) => ABSENT,
        Err(_) => UNMARKED,
    }
}

impl Fields for View<'_> {
    fn field(&self, name: &str) -> Result<Current<'_>> {
        match self.collection.fields.by_name(name) {
            Some(field) => self.field_of(field),
            None => Ok(Current::Borrowed(ValueRef::Null)),
        }
    }

    // Inlined into the filter that asks, which reads the value where this
    // writes it: returned through memory instead, a byte at a time, the
    // value took a filter on one string field a fifth longer.
    #[inline(always)]
    fn field_of<'a>(&'a self, field: &'a FieldDef) -> Result<Current<'a>> {
        let damaged = |reason: &str| {
            self.source
                .corrupted(format!("an object of `{}`: {reason}", self.collection.name))
        };
        // The field the walk marked is read where its mark says, rather
        // than found by stepping over the fields before it.
        let found = match self.mark {
            mark if self.marked != Some(field.id) || mark == UNMARKED => {
                codec::find_field(self.record, field.id)
            }
            ABSENT => Ok(None),
            at => codec::field_at(self.record, usize::from(at)).map(Some),
        }
        .map_err(damaged)?;

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

/// Whether `test` holds for `value`, null or not, as a filter's test of one
/// value has it hold.
#[inline(always)]
fn direct(test: &Test<'_>, value: ValueRef<'_>) -> bool {
    match test {
        Test::IsNull => value.is_null(),
        // The commonest test, made here rather than through a call to
        // `passes` for every object a walk tests.
        Test::Compare(Op::Eq, other) => !value.is_null() && value.equals(other),
        test => !value.is_null() && passes(test, value),
    }
}

/// Whether `test` holds for the value `value`, which is not null.
fn passes(test: &Test, value: ValueRef<'_>) -> bool {
    match test {
        Test::Compare(Op::Eq, other) | Test::Element(other) => value.equals(other),
        Test::Compare(Op::Ne, other) => !value.equals(other),
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
        // Found among the string's bytes, which are checked to be UTF-8 only
        // where it is found: a check of every string a walk tested took a
        // tenth of its time.
        Test::Substring(finder) => value.string_bytes().is_some_and(|bytes| {
            finder.find(bytes).is_some() && std::str::from_utf8(bytes).is_ok()
        }),
        Test::StartsWith(text) => value
            .string_bytes()
            .is_some_and(|value| value.starts_with(text.as_bytes())),
        Test::EndsWith(text) => value
            .string_bytes()
            .is_some_and(|value| value.ends_with(text.as_bytes())),
        Test::IsNull => value.is_null(),
    }
}

/// What a run walks to find a plan's objects.
#[derive(Clone, Copy)]
enum Route<'r, 's> {
    /// An access, giving every key it finds.
    Access(&'r Access<'s>),
    /// An access, giving only the keys a set holds.
    Through(&'r Access<'s>, &'r KeySet),
    /// The keys of a set, in their order.
    Keys(&'r KeySet),
}

/// Gives `visit` the key of each object `route` finds, in the route's order,
/// with its record where the walk has it, until `visit` says to stop.
///
/// A run that walks the plan's access calls [`walk_access`] itself rather
/// than through this function, which made a walk of every record take a few
/// percent longer.
fn walk(
    source: &dyn Source,
    plan: &Plan<'_>,
    route: Route<'_, '_>,
    visit: &mut impl FnMut(&[u8], Option<Lent<'_>>) -> Result<bool>,
) -> Result<()> {
    match route {
        Route::Access(access) => walk_access(source, plan, access, visit),
        Route::Through(access, set) => walk_access(source, plan, access, &mut |key, walked| {
            if set.contains(key) {
                visit(key, walked)
            } else {
                Ok(false)
            }
        }),
        Route::Keys(set) => {
            let tree = records(plan.collection.id);
            let mut seeker = source.seeker_in(&tree)?;

            for at in 0..set.len() {
                let key = set.get(at);
                let mut stop = false;

                // The records are looked up in key order, each from where the
                // last was found, and lent where they lie.
                let found = seeker.get_with(key, &mut |record| {
                    stop = visit(key, Some(Lent::unmarked(record)))?;

                    Ok(())
                })?;

                if !found {
                    return Err(source.corrupted(format!(
                        "an index of `{}` names an object that is not there",
                        plan.collection.name
                    )));
                }

                if stop {
                    break;
                }
            }

            Ok(())
        }
    }
}

/// Walks the keys the access names, in its order, reading each object once.
/// `visit` gets a primary key and, when the walk read it, the record, both
/// borrowed; it returns whether to stop.
///
/// Generic over `visit`, which every object read goes through: a call
/// through a pointer for each object was a tenth of a walk over every
/// record that tests one field.
fn walk_access(
    source: &dyn Source,
    plan: &Plan<'_>,
    access: &Access<'_>,
    visit: &mut impl FnMut(&[u8], Option<Lent<'_>>) -> Result<bool>,
) -> Result<()> {
    let records = records(plan.collection.id);

    match access {
        Access::Records { range, backward } => {
            let marked = plan.marked.map(|field| field.id);

            source
                .range_in(&records, as_ref(&range.0), as_ref(&range.1), *backward)?
                .for_each_marked(
                    marked,
                    &|record| marked.map_or(UNMARKED, |id| field_mark(record, id)),
                    &mut |key, record, mark| visit(key, Some(Lent { record, mark })),
                )?;
        }
        Access::Keys { keys, backward } => {
            for at in 0..keys.len() {
                let key = &keys[in_order(at, keys.len(), *backward)];
                let mut stop = false;

                // The record is lent where it lies, as a walk lends it.
                source.get_in_with(&records, key, &mut |record| {
                    stop = visit(key, Some(Lent::unmarked(record)))?;

                    Ok(())
                })?;

                if stop {
                    break;
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
                    // up, not walked, and the key lent rather than copied.
                    let mut stop = false;

                    source.get_in_with(&tree, value, &mut |key| {
                        stop = give(key, &mut seen)?;

                        Ok(())
                    })?;

                    stop
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

/// Runs `plan` and returns the objects it finds, in its order, with their
/// fields put in their places by `order`, the collection's, if the caller
/// knows it. Only the objects in the result are decoded whole.
pub(crate) fn objects(
    source: &dyn Source,
    plan: &Plan<'_>,
    order: Option<&NameOrder>,
) -> Result<Vec<Object>> {
    let mut objects = Vec::new();
    let mut decoder = objects::Decoder::new(plan.collection, order);

    found(source, plan, &mut |record| {
        objects.push(decoder.decode(source, &record)?);

        Ok(())
    })?;

    Ok(objects)
}

/// Gives `take` the record of the one object `ir` finds, when it asks for
/// one value of the primary key or of a unique index and nothing else, and
/// returns whether it was such a query: found with two lookups at most,
/// and no plan. Any other query, and one with a value of another type than
/// its field's or null, is left to the planner, which gives the same answer
/// or the error.
///
/// A lookup by one value is what an application asks most often, and
/// planning it, then walking the plan's one key, took a quarter of it.
pub(crate) fn point(
    source: &dyn Source,
    collection: &CollectionDef,
    ir: &Ir,
    parameters: &[Value],
    take: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    if !ir.sort.is_empty() || ir.offset != 0 || ir.limit == Some(0) {
        return Ok(false);
    }

    let (path, value) = match &ir.filter {
        Some(Expr::Test {
            op: Op::Eq,
            path,
            values,
        }) if values.len() == 1 => (path, &values[0]),
        Some(Expr::Prepared {
            op: Op::Eq,
            path,
            values,
        }) if values.len() == 1 => match &values[0] {
            Operand::Value(value) => (path, value),
            Operand::Param(index) => match parameters.get(*index) {
                Some(value) => (path, value),
                None => return Ok(false),
            },
        },
        _ => return Ok(false),
    };
    let [name] = path.as_slice() else {
        return Ok(false);
    };
    let Some(field) = collection.fields.by_name(name) else {
        return Ok(false);
    };

    if !matches!(
        (&field.kind, value),
        (Kind::Int, Value::Int(_))
            | (Kind::String, Value::String(_))
            | (Kind::Bytes, Value::Bytes(_))
    ) {
        return Ok(false);
    }

    let records = records(collection.id);

    if field.id == collection.key {
        objects::with_key_bytes(collection, value, |key| {
            source.get_in_with(&records, key, take)
        })?;

        return Ok(true);
    }

    let Some(index) = collection
        .indexes
        .iter()
        .find(|index| index.unique && index.field == field.id)
    else {
        return Ok(false);
    };
    // A unique index keys a value that is not null by the value alone.
    key::with_encoded(value, |entry| {
        source.get_in_with(&index_tree(index.id), entry, &mut |key| {
            if source.get_in_with(&records, key, take)? {
                Ok(())
            } else {
                Err(source.corrupted(format!(
                    "an index of `{}` names an object that is not there",
                    collection.name
                )))
            }
        })
    })
    .map_err(|reason| Error::Internal {
        message: reason.to_owned(),
    })??;

    Ok(true)
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
    if plan.limit == Some(0) {
        return Ok(());
    }

    let strategy = strategy(source, plan, false)?;

    if let Strategy::Try {
        access,
        part,
        budget,
    } = &strategy
    {
        if found_by(source, plan, take, &strategy, Some(*budget))? {
            return Ok(());
        }

        // The access found more than the budget, and gave nothing yet.
        let kept = plan.offset.saturating_add(plan.limit.unwrap_or(u64::MAX));
        let turned = turn(
            source,
            plan,
            access,
            gather(source, plan.collection, part)?,
            kept,
        )?;

        found_by(source, plan, take, &turned, None)?;

        return Ok(());
    }

    found_by(source, plan, take, &strategy, None)?;

    Ok(())
}

/// [`found`] by `strategy`. With a `budget`, a walk that is not in the
/// query's order stops once it has found more objects than that, and
/// returns false, having given nothing.
fn found_by(
    source: &dyn Source,
    plan: &Plan<'_>,
    take: &mut Take<'_>,
    strategy: &Strategy<'_, '_>,
    budget: Option<u64>,
) -> Result<bool> {
    let limit = plan.limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit).unwrap_or(usize::MAX)
    });
    let offset = usize::try_from(plan.offset).unwrap_or(usize::MAX);
    let (route, skip, ordered) = strategy.route(plan);
    let reader = Reader::skipping(source, plan, skip);
    // Whether any of the filter is left to test on the objects read.
    let tested = plan.filter.is_some() && !reader.answered;

    if ordered {
        let mut skipped = 0;
        let mut taken = 0;

        let mut visit = |key: &[u8], walked: Option<Lent<'_>>| -> Result<bool> {
            // With no filter left, every object the walk gives meets it, so
            // one the offset skips is skipped without reading its record: a
            // page deep in an index's order reads the page's records alone.
            if !tested && skipped < offset {
                skipped += 1;

                return Ok(false);
            }

            let mut stop = false;
            let mut each = |lent: Lent<'_>| {
                if !meets(&reader, plan, Some(lent), None)? {
                    return Ok(());
                }

                if skipped < offset {
                    skipped += 1;

                    return Ok(());
                }

                take(Cow::Borrowed(lent.record))?;
                taken += 1;
                stop = taken >= limit;

                Ok(())
            };

            // A record the walk lent is tested here, not through a pointer.
            match walked {
                Some(lent) => each(lent)?,
                None => with_record(&reader, plan, key, None, &mut each)?,
            }

            Ok(stop)
        };

        match route {
            Route::Access(access) => walk_access(source, plan, access, &mut visit)?,
            route => walk(source, plan, route, &mut visit)?,
        }

        return Ok(true);
    }

    let keep = offset.saturating_add(limit);
    // How many objects the walk found, against the budget.
    let mut seen = 0u64;
    let mut over = false;
    let compare = |a: &Found, b: &Found| order(plan, a, b);
    let mut found: Vec<Found> = Vec::new();
    // The values an object sorts by, filled again for each object tested.
    let mut values = Vec::new();
    // Once the kept objects have been cut down to `keep`, the last of them:
    // an object that sorts after it cannot be in the result, and is not
    // copied.
    let mut trimmed = false;
    // Without a sort the result is in primary key order, so a key alone says
    // whether its object can still be in the result, and one that cannot is
    // not read. When the index answers the filter too, its entries decide
    // which objects are found, and only the records of the result are read.
    let by_key = plan.sort.is_empty();
    let deferred = by_key
        && !tested
        && matches!(
            route,
            Route::Access(Access::Index { .. }) | Route::Through(Access::Index { .. }, _)
        );

    let mut visit = |key: &[u8], record: Option<Lent<'_>>| -> Result<bool> {
        if let Some(budget) = budget {
            seen += 1;

            if seen > budget {
                over = true;

                return Ok(true);
            }
        }

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
            let bound = bound.map(|bound| (bound.sort.as_slice(), bound.key.as_slice()));
            let Some(hit) = read(&reader, plan, key, record, bound, &mut values)? else {
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
    };

    match route {
        Route::Access(access) => walk_access(source, plan, access, &mut visit)?,
        route => walk(source, plan, route, &mut visit)?,
    }

    if over {
        return Ok(false);
    }

    found.sort_by(compare);

    for hit in found.into_iter().skip(offset).take(limit) {
        if deferred {
            with_record(&reader, plan, &hit.key, None, &mut |lent| {
                take(Cow::Borrowed(lent.record))
            })?;
        } else {
            take(Cow::Owned(hit.record))?;
        }
    }

    Ok(true)
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

/// The object with primary key `key`, whose record the walk may have lent
/// already, if it meets the plan's filter, with the values it sorts by, and
/// only if it sorts before `bound`, the values and the key of the last object
/// kept once the kept ones have been cut down. The filter and the sort read
/// the fields they need from the record, and the sort's values go into
/// `values`; the values, the key and the record are taken or copied only for
/// an object that is kept.
fn read(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    key: &[u8],
    walked: Option<Lent<'_>>,
    bound: Option<(&[Value], &[u8])>,
    values: &mut Vec<Value>,
) -> Result<Option<Found>> {
    let mut hit = None;

    with_record(reader, plan, key, walked, &mut |lent| {
        if let Some(bound) = bound {
            if !sorts_before(reader, plan, lent, key, bound)? {
                return Ok(());
            }
        }

        if meets(reader, plan, Some(lent), Some(values))? {
            hit = Some(Found {
                sort: std::mem::take(values),
                key: key.to_vec(),
                record: lent.record.to_vec(),
            });
        }

        Ok(())
    })?;

    Ok(hit)
}

/// Gives `visit` the record of the object with primary key `key`: the one
/// the walk lent, or else the object's record, read where it lies through
/// the reader's lookups of records. Either way it is borrowed, so that a
/// record the query does not keep is never copied.
fn with_record(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    key: &[u8],
    walked: Option<Lent<'_>>,
    visit: &mut dyn FnMut(Lent<'_>) -> Result<()>,
) -> Result<()> {
    if let Some(lent) = walked {
        return visit(lent);
    }

    let visit = &mut |record: &[u8]| visit(Lent::unmarked(record));
    let tree = records(plan.collection.id);
    let mut records = reader.records.borrow_mut();
    let found = match &mut *records {
        Records::Many(seeker) => seeker.get_with(key, visit)?,
        Records::One => {
            let mut seeker = reader.source.seeker_in(&tree)?;
            let found = seeker.get_with(key, visit)?;

            *records = Records::Many(seeker);
            found
        }
        Records::None => {
            *records = Records::One;
            reader.source.get_in_with(&tree, key, visit)?
        }
    };

    if found {
        Ok(())
    } else {
        Err(reader.source.corrupted(format!(
            "an index of `{}` names an object that is not there",
            plan.collection.name
        )))
    }
}

/// Whether the object whose record is `record` and whose key is `key` sorts
/// before the object that sorts by `bound`'s values and has its key: what
/// [`order_of`] says of the values [`meets`] would give it, with each value
/// compared where the record holds it. A sort with a limit asks this of every
/// object once it has kept enough of them, and nearly every one it reads
/// after the first few does not sort before; its values are made only for
/// an object that does.
fn sorts_before(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    lent: Lent<'_>,
    key: &[u8],
    bound: (&[Value], &[u8]),
) -> Result<bool> {
    let view = View::new(reader, plan, lent);
    #[cfg(test)]
    let object = WHOLE
        .with(std::cell::Cell::get)
        .then(|| objects::decode(reader.source, plan.collection, lent.record))
        .transpose()?;
    #[cfg(test)]
    let fields: &dyn Fields = match &object {
        Some(object) => object,
        None => &view,
    };
    #[cfg(not(test))]
    let fields = &view;

    for ((path, descending), other) in plan.sort.iter().zip(bound.0) {
        // Null where the path reaches no value, as `value_at` gives it.
        let mut order = ValueRef::Null.compare(other);

        reader.any_in(fields, path, &mut |value| {
            order = value.compare(other);
            true
        })?;

        let order = if *descending { order.reverse() } else { order };

        if order.is_ne() {
            return Ok(order.is_lt());
        }
    }

    Ok(key < bound.1)
}

/// Whether the object whose record is `record` meets the plan's filter.
/// When it does, and `sort` is given, the values it sorts by replace what
/// `sort` held: one vector serves every object a query tests, where one made
/// for each and dropped with the objects not kept cost most of a sort's
/// allocations.
fn meets(
    reader: &Reader<'_>,
    plan: &Plan<'_>,
    lent: Option<Lent<'_>>,
    mut sort: Option<&mut Vec<Value>>,
) -> Result<bool> {
    let lent = lent.unwrap_or_default();

    if let Some(values) = sort.as_deref_mut() {
        values.clear();
    }

    let sorting = sort.is_some() && !plan.sort.is_empty();
    let filter = if reader.answered {
        None
    } else {
        plan.filter.as_ref()
    };

    if filter.is_none() && !sorting {
        return Ok(true);
    }

    let view = View::new(reader, plan, lent);
    #[cfg(test)]
    let object = WHOLE
        .with(std::cell::Cell::get)
        .then(|| objects::decode(reader.source, plan.collection, lent.record))
        .transpose()?;
    #[cfg(test)]
    let fields: &dyn Fields = match &object {
        Some(object) => object,
        None => &view,
    };
    #[cfg(not(test))]
    let fields = &view;

    if let Some(filter) = filter {
        // A filter that is one test of a field of the collection itself is
        // made here rather than through the filter's tree, which passed the
        // test to the field through a pointer: a walk over every object
        // makes it of each.
        let held = match filter {
            Cond::Test {
                path:
                    Resolved {
                        steps,
                        direct: Some(field),
                        many: false,
                        ..
                    },
                test,
            } if steps.len() == 1 => match fields.field_of(field)? {
                Current::Borrowed(value) => direct(test, value),
                Current::Owned(value) => direct(test, ValueRef::Value(&value)),
            },
            _ => reader.holds_left(filter, fields)?,
        };

        if !held {
            return Ok(false);
        }
    }

    if let Some(values) = sort {
        for (path, _) in &plan.sort {
            values.push(reader.value_at(fields, path)?);
        }
    }

    Ok(true)
}

/// Runs `plan` and counts the objects it finds, after its offset and within
/// its limit.
pub(crate) fn count(source: &dyn Source, plan: &Plan<'_>) -> Result<u64> {
    let strategy = strategy(source, plan, true)?;
    let found = match &strategy {
        Strategy::Plain | Strategy::Try { .. } => count_plain(source, plan)?,
        // A set that answers the whole filter is the objects found.
        Strategy::Keys { set, skip }
            if !plan.filter.as_ref().is_some_and(|f| !answered(f, *skip)) =>
        {
            set.len() as u64
        }
        Strategy::Keys { .. } | Strategy::Through { .. } => {
            let (route, skip, _) = strategy.route(plan);
            let reader = Reader::skipping(source, plan, skip);
            let mut found = 0u64;

            walk(source, plan, route, &mut |key, walked| {
                with_record(&reader, plan, key, walked, &mut |lent| {
                    found += u64::from(meets(&reader, plan, Some(lent), None)?);

                    Ok(())
                })?;

                Ok(false)
            })?;

            found
        }
    };
    let found = found.saturating_sub(plan.offset);

    Ok(plan.limit.map_or(found, |limit| found.min(limit)))
}

/// The number of objects `plan`'s access and filter find, before its offset
/// and limit, without a set of keys.
fn count_plain(source: &dyn Source, plan: &Plan<'_>) -> Result<u64> {
    Ok(if plan.filter.is_none() && !plan.exact {
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

                walk(source, plan, Route::Access(&plan.access), &mut |_, _| {
                    found += 1;

                    Ok(false)
                })?;

                found
            }
        }
    } else {
        let reader = Reader::new(source);
        let mut found = 0u64;

        walk(
            source,
            plan,
            Route::Access(&plan.access),
            &mut |key, walked| {
                with_record(&reader, plan, key, walked, &mut |lent| {
                    found += u64::from(meets(&reader, plan, Some(lent), None)?);

                    Ok(())
                })?;

                Ok(false)
            },
        )?;

        found
    })
}

/// Whether every term of `filter` is one `skip` marks.
fn answered(filter: &Cond<'_>, skip: u64) -> bool {
    skip != 0 && (0..terms_of(Some(filter)).len()).all(|position| skipped(skip, position))
}

/// Whether `skip` marks the term at `position`.
fn skipped(skip: u64, position: usize) -> bool {
    position < 64 && skip & 1 << position != 0
}

/// How a run finds a plan's objects (`design/objects.md`, "Sets of keys").
enum Strategy<'p, 's> {
    /// The plan's access, each object tested against the plan's filter.
    Plain,
    /// The objects of a set of keys gathered from the parts of the filter
    /// that indexes answer, in key order, each tested against the terms of
    /// the filter the set does not answer, which `skip` leaves.
    Keys { set: KeySet, skip: u64 },
    /// The walk in the sort's order, taking only the keys in a set gathered
    /// as for `Keys`, so that a limit stops the walk.
    Through {
        access: &'p Access<'s>,
        set: KeySet,
        skip: u64,
    },
    /// The plan's access, unless it finds more than `budget` objects: then
    /// `Through` the walk `access`, with the keys of `part`, the access's.
    Try {
        access: &'p Access<'s>,
        part: &'p Part<'s>,
        budget: u64,
    },
}

impl<'p, 's> Strategy<'p, 's> {
    /// The route a run walks, the terms of the filter it leaves out, and
    /// whether the walk gives the objects in the query's order.
    fn route<'r>(&'r self, plan: &'r Plan<'s>) -> (Route<'r, 's>, u64, bool)
    where
        'p: 'r,
    {
        match self {
            Strategy::Plain | Strategy::Try { .. } => {
                (Route::Access(&plan.access), 0, plan.ordered)
            }
            Strategy::Keys { set, skip } => (Route::Keys(set), *skip, plan.sort.is_empty()),
            Strategy::Through { access, set, skip } => (Route::Through(access, set), *skip, true),
        }
    }
}

/// What a run weighs its ways by, in units of walking one entry of a tree:
/// looking an object's record up by its key and testing it, and finding a
/// key's place in a tree from the root. On an Apple M1 Max, walking an
/// entry of an index took about 30 ns, looking a record up about 350 ns, and
/// a descent about a microsecond.
const RECORD: u64 = 12;
const SEEK: u64 = 30;

/// A walk of every record reads each where it lies, at about a third of a
/// lookup's cost.
const WALKED: u64 = 4;

/// A part that holds more than this many times the keys the smallest holds
/// is left to be tested on the objects read, rather than gathered.
const SPREAD: u64 = 8;

/// The ways the tests make a run take, rather than weighing them.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Forced {
    Weigh,
    Plain,
    Keys,
    Through,
}

#[cfg(test)]
thread_local! {
    static FORCED: std::cell::Cell<Forced> = const { std::cell::Cell::new(Forced::Weigh) };
}

/// Runs `run` with every query taking `way`, where it can.
#[cfg(test)]
pub(crate) fn forced<T>(way: Forced, run: impl FnOnce() -> T) -> T {
    FORCED.with(|forced| forced.set(way));

    let result = run();

    FORCED.with(|forced| forced.set(Forced::Weigh));
    result
}

/// A part of a plan, with about how many keys it holds, and what gathering
/// them costs.
struct Weighed<'p, 's> {
    /// The term of the plan's filter the part is, or `None` for what the
    /// access answers.
    position: Option<usize>,
    part: &'p Part<'s>,
    size: u64,
    cost: u64,
}

/// How a run finds `plan`'s objects: the access the plan chose, or a set of
/// the keys the parts of its filter that indexes answer come to, walked
/// alone or through the walk in the sort's order, whichever the sizes of
/// the indexes' ranges make cheapest.
fn strategy<'p, 's>(
    source: &dyn Source,
    plan: &'p Plan<'s>,
    counting: bool,
) -> Result<Strategy<'p, 's>> {
    #[cfg(test)]
    let forced = FORCED.with(std::cell::Cell::get);
    #[cfg(test)]
    let weigh = forced == Forced::Weigh;
    #[cfg(not(test))]
    let weigh = true;

    let through = plan
        .sort_walk
        .as_ref()
        .filter(|_| plan.limit.is_some() && !counting);

    #[cfg(test)]
    if forced == Forced::Plain {
        return Ok(Strategy::Plain);
    }

    let collection = plan.collection;
    let take = plan.limit.map(|limit| plan.offset.saturating_add(limit));

    if plan.parts.is_empty() && (through.is_none() || plan.access_part.is_none()) {
        return Ok(Strategy::Plain);
    }

    if weigh {
        // A lookup of a few keys, or of a few values of a unique index, is
        // as cheap as a set could make it.
        let few = match &plan.access {
            Access::Keys { keys, .. } => keys.len() <= 16,
            Access::Index {
                index,
                ranges,
                values: true,
                ..
            } => index.unique && ranges.len() <= 16,
            _ => false,
        };

        // A count the access answers alone reads no record already.
        if few || (counting && plan.exact) {
            return Ok(Strategy::Plain);
        }
    }

    if let (true, Some(access), Some(take), Some(part)) =
        (plan.parts.is_empty(), through, take, &plan.access_part)
    {
        #[cfg(test)]
        if !weigh {
            return turn(
                source,
                plan,
                access,
                gather(source, collection, part)?,
                take,
            );
        }

        // Only the walk in the sort's order is left to weigh, which pays
        // when the access finds many more objects than the limit takes, and
        // only walking the access tells that. So the run walks it, and turns
        // to the walk in the sort's order once it has found eight times what
        // the limit takes: a query whose access finds a few, such as the
        // newest posts of one author, costs nothing more.
        return Ok(Strategy::Try {
            access,
            part,
            budget: take.saturating_mul(SPREAD).max(SPREAD),
        });
    }

    let total = source.len_in(&records(collection.id))?.max(1);
    let access = access_size(source, collection, &plan.access)?;
    let cap = access.saturating_mul(SPREAD).saturating_add(1);
    let mut weighed = Vec::new();

    for (position, part) in &plan.parts {
        if *position < 64 {
            let (size, cost) = weight(source, collection, part, cap)?;

            weighed.push(Weighed {
                position: Some(*position),
                part,
                size,
                cost,
            });
        }
    }

    if let Some(part) = &plan.access_part {
        weighed.push(Weighed {
            position: None,
            part,
            size: access,
            cost: weight(source, collection, part, 0)?.1.max(access),
        });
    }

    weighed.sort_by_key(|weighed| weighed.size);

    let fewest = weighed.first().map_or(0, |weighed| weighed.size);
    let most = fewest.saturating_mul(SPREAD).max(SPREAD);
    let (chosen, left): (Vec<_>, Vec<_>) = weighed
        .into_iter()
        .partition(|weighed| !weigh || weighed.size <= most);
    // A run that leaves the access gathers what the access answered, chosen
    // or not, since the filter no longer holds those terms.
    let access_left = left.into_iter().find(|weighed| weighed.position.is_none());
    let skip = chosen
        .iter()
        .filter_map(|weighed| weighed.position)
        .fold(0u64, |skip, position| skip | 1 << position);
    let tested = plan
        .filter
        .as_ref()
        .is_some_and(|filter| !answered(filter, skip));
    let gathered = chosen
        .iter()
        .chain(access_left.as_ref())
        .map(|weighed| weighed.cost)
        .fold(0u64, u64::saturating_add);
    // About how many objects the set holds, if the parts were independent.
    let result = chosen
        .iter()
        .chain(access_left.as_ref())
        .skip(1)
        .fold(fewest, |kept, weighed| {
            kept.saturating_mul(weighed.size) / total
        })
        .max(1);
    let per_object = match &plan.access {
        Access::Records { .. } => WALKED,
        _ => 1 + RECORD,
    };
    // A walk in the query's order stops at the limit.
    let plain = match take {
        Some(take) if plan.ordered => access.min(take.saturating_mul(access) / result),
        _ => access,
    }
    .saturating_mul(per_object);
    let keys = gathered.saturating_add(if counting && !tested {
        0
    } else {
        result.saturating_mul(RECORD)
    });
    let walked = through.zip(take).map(|(_, take)| {
        let walked = total.min(take.saturating_mul(total) / result);

        gathered
            .saturating_add(walked)
            .saturating_add(take.saturating_mul(RECORD))
    });

    #[cfg(test)]
    let (plain, keys, walked) = match forced {
        Forced::Keys => (u64::MAX, 0, None),
        Forced::Through if walked.is_some() => (u64::MAX, u64::MAX, Some(0)),
        Forced::Through => (u64::MAX, 0, None),
        _ => (plain, keys, walked),
    };

    if plain <= keys && walked.is_none_or(|walked| plain <= walked) {
        return Ok(Strategy::Plain);
    }

    // The smallest part first, then each other kept to the keys found in
    // it, so that no part larger than the first is held.
    let mut parts = chosen
        .iter()
        .chain(access_left.as_ref())
        .map(|weighed| weighed.part);
    let mut set = match parts.next() {
        Some(part) => gather(source, collection, part)?,
        None => return Ok(Strategy::Plain),
    };

    for part in parts {
        if set.is_empty() {
            break;
        }

        set = keep(source, collection, part, &set)?;
    }

    Ok(match (through, walked) {
        (Some(access), Some(walked)) if walked < keys => Strategy::Through { access, set, skip },
        // A count the set answers whole needs only its size.
        _ if counting && !tested => Strategy::Keys { set, skip },
        _ => Strategy::Keys {
            set: set.in_order(),
            skip,
        },
    })
}

/// How a run whose access found more than its budget goes on: through
/// `access`, the walk in the sort's order, taking only the keys in `set`,
/// the access's, or reading the set's objects, whichever is cheaper for the
/// `take` objects the query keeps.
fn turn<'p, 's>(
    source: &dyn Source,
    plan: &Plan<'s>,
    access: &'p Access<'s>,
    set: KeySet,
    take: u64,
) -> Result<Strategy<'p, 's>> {
    let size = set.len() as u64;
    let total = source.len_in(&records(plan.collection.id))?.max(1);
    let walked = total.min(take.saturating_mul(total) / size.max(1));
    let through = walked.saturating_add(take.saturating_mul(RECORD));
    let keys = size.saturating_mul(RECORD);
    #[cfg(test)]
    let (through, keys) = match FORCED.with(std::cell::Cell::get) {
        Forced::Keys => (u64::MAX, 0),
        Forced::Through => (0, u64::MAX),
        _ => (through, keys),
    };

    Ok(if through < keys {
        Strategy::Through {
            access,
            set,
            skip: 0,
        }
    } else {
        Strategy::Keys {
            set: set.in_order(),
            skip: 0,
        }
    })
}

/// How many objects `access` finds before the filter is tested.
fn access_size(
    source: &dyn Source,
    collection: &CollectionDef,
    access: &Access<'_>,
) -> Result<u64> {
    let count = |tree: &str, ranges: &[Range]| -> Result<u64> {
        let mut size = 0u64;

        for range in ranges {
            size += source
                .range_in(tree, as_ref(&range.0), as_ref(&range.1), false)?
                .count_entries()?;
        }

        Ok(size)
    };

    match access {
        Access::Records {
            range: (Bound::Unbounded, Bound::Unbounded),
            ..
        } => source.len_in(&records(collection.id)),
        Access::Records { range, .. } => {
            count(&records(collection.id), std::slice::from_ref(range))
        }
        Access::Keys { keys, .. } => Ok(keys.len() as u64),
        Access::Index { index, ranges, .. } => count(&index_tree(index.id), ranges),
    }
}

/// About how many keys `part`, of `collection`, holds, counted up to about
/// `cap` and no further, and what gathering them costs. With a `cap` of 0,
/// only the cost of a part of known keys or ranges, without counting.
fn weight(
    source: &dyn Source,
    collection: &CollectionDef,
    part: &Part<'_>,
    cap: u64,
) -> Result<(u64, u64)> {
    let count = |tree: &str, ranges: &[Range]| -> Result<u64> {
        let mut size = 0u64;

        for range in ranges {
            if size >= cap {
                break;
            }

            size += source
                .range_in(tree, as_ref(&range.0), as_ref(&range.1), false)?
                .count_entries_up_to(cap - size)?;
        }

        Ok(size)
    };

    Ok(match part {
        Part::Records(ranges) => {
            let size = count(&records(collection.id), ranges)?;

            (size, size)
        }
        Part::Keys(keys) => {
            let size = keys.len() as u64;

            (size, size.saturating_mul(SEEK))
        }
        Part::Index { index, ranges } => {
            let size = count(&index_tree(index.id), ranges)?;

            (size, size.saturating_add(ranges.len() as u64 * SEEK))
        }
        Part::Linked {
            index,
            target,
            inner,
        } => {
            // Each object of the linked collection is linked to by about as
            // many objects as the index has entries for each.
            let entries = source.len_in(&index_tree(index.id))?;
            let fanout = (entries / source.len_in(&records(target.id))?.max(1)).max(1);
            let (found, cost) = weight(source, target, inner, cap / fanout + 1)?;
            let size = found.saturating_mul(fanout);
            // Each object found is looked up in the index, unless a walk of
            // the whole index is cheaper.
            let lookups = found.saturating_mul(SEEK).min(entries);

            (size, cost.saturating_add(lookups).saturating_add(size))
        }
        Part::And(parts) => {
            let mut size = u64::MAX;
            let mut cost = 0u64;

            for part in parts {
                let (found, gathering) = weight(source, collection, part, cap)?;

                size = size.min(found);
                cost = cost.saturating_add(gathering);
            }

            (size.min(cap.max(1)), cost)
        }
        Part::Or(parts) => {
            let mut size = 0u64;
            let mut cost = 0u64;

            for part in parts {
                let (found, gathering) = weight(source, collection, part, cap)?;

                size = size.saturating_add(found);
                cost = cost.saturating_add(gathering);
            }

            (size, cost)
        }
    })
}

/// The keys of the objects of `collection` that `part` finds.
fn gather(source: &dyn Source, collection: &CollectionDef, part: &Part<'_>) -> Result<KeySet> {
    if let Part::And(parts) = part {
        let mut parts = parts.iter();
        let mut set = match parts.next() {
            Some(first) => gather(source, collection, first)?,
            None => return Ok(KeySet::default()),
        };

        for part in parts {
            if set.is_empty() {
                break;
            }

            set = keep(source, collection, part, &set)?;
        }

        return Ok(set);
    }

    let mut set = KeySet::default();

    each_key(source, collection, part, &mut |key| {
        set.push(key);

        Ok(())
    })?;

    Ok(set.seal())
}

/// The keys of `set` that `part` finds too.
fn keep(
    source: &dyn Source,
    collection: &CollectionDef,
    part: &Part<'_>,
    set: &KeySet,
) -> Result<KeySet> {
    if let Part::And(parts) = part {
        let mut kept = None;

        for part in parts {
            kept = Some(keep(
                source,
                collection,
                part,
                kept.as_ref().unwrap_or(set),
            )?);
        }

        return Ok(kept.unwrap_or_default());
    }

    let mut kept = KeySet::default();

    each_key(source, collection, part, &mut |key| {
        if set.contains(key) {
            kept.push(key);
        }

        Ok(())
    })?;

    Ok(kept.seal())
}

/// Gives `give` the key of each object of `collection` that `part` finds,
/// once or more, in no order to rely on.
fn each_key(
    source: &dyn Source,
    collection: &CollectionDef,
    part: &Part<'_>,
    give: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    match part {
        Part::Records(ranges) => {
            let tree = records(collection.id);

            for range in ranges {
                source
                    .range_in(&tree, as_ref(&range.0), as_ref(&range.1), false)?
                    .for_each(&mut |key, _| {
                        give(key)?;

                        Ok(false)
                    })?;
            }
        }
        Part::Keys(keys) => {
            let tree = records(collection.id);

            for key in keys {
                if source.get_in_with(&tree, key, &mut |_| Ok(()))? {
                    give(key)?;
                }
            }
        }
        Part::Index { index, ranges } => {
            let tree = index_tree(index.id);

            for range in ranges {
                walk_entries(source, index, &tree, range, &mut |key| {
                    give(key)?;

                    Ok(false)
                })?;
            }
        }
        Part::Linked {
            index,
            target,
            inner,
        } => {
            let targets = gather(source, target, inner)?;

            if targets.is_empty() {
                return Ok(());
            }

            let tree = index_tree(index.id);

            // An entry of a link's index starts with the linked object's key,
            // encoded as the linked collection's records are keyed.
            if (targets.len() as u64).saturating_mul(SEEK) > source.len_in(&tree)? {
                // Fewer entries in the whole index than lookups of each: the
                // index is walked once, and the entries of the set's keys
                // kept.
                source
                    .range_in(&tree, Bound::Unbounded, Bound::Unbounded, false)?
                    .for_each(&mut |entry, stored| {
                        let (value, key) = entry_parts(source, index, entry, stored)?;

                        if targets.contains(value) {
                            give(key)?;
                        }

                        Ok(false)
                    })?;
            } else {
                for at in 0..targets.len() {
                    let value = targets.get(at);
                    let range = (
                        Bound::Included(value.to_vec()),
                        key::after_prefix(value).map_or(Bound::Unbounded, Bound::Excluded),
                    );

                    walk_entries(source, index, &tree, &range, &mut |key| {
                        give(key)?;

                        Ok(false)
                    })?;
                }
            }
        }
        Part::And(_) => {
            let set = gather(source, collection, part)?;

            for at in 0..set.len() {
                give(set.get(at))?;
            }
        }
        Part::Or(parts) => {
            for part in parts {
                each_key(source, collection, part, give)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::same_bytes;

    /// Byte strings of every length up to past two words are the same exactly
    /// when they are equal, whichever byte differs.
    #[test]
    fn byte_strings_compare_by_words_as_they_do_by_bytes() {
        for len in 0..40u8 {
            let a: Vec<u8> = (0..len).map(|n| n ^ 0x5A).collect();
            let len = usize::from(len);

            assert!(same_bytes(&a, &a.clone()));

            if len > 0 {
                assert!(!same_bytes(&a, &a[..len - 1]), "{len} bytes and one fewer");
            }

            for at in 0..len {
                let mut b = a.clone();

                b[at] ^= 1;

                assert!(!same_bytes(&a, &b), "{len} bytes, differing at {at}");
            }
        }
    }
}
