//! Reading and writing the objects of a collection, and keeping its indexes
//! in step with them (`design/objects.md`, "Storage", "Indexes" and "Writing
//! objects").
//!
//! Every write checks everything that can refuse it, the object's types, its
//! key, the unique indexes and the lengths of every key it adds, before it
//! changes an index. A put or an update stores its record before the unique
//! indexes are checked, and stores the record it replaced again if one
//! refuses it. A refused write leaves the transaction holding what it held
//! and able to commit.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::format::object::codec::{self, FieldRef};
use crate::format::object::key;
use crate::format::object::schema::{
    CollectionDef, FieldDef, IndexDef, Kind, OpenSchema, StoredSchema,
};
use crate::format::object::{Object, Value};
use crate::txn::{Range, ReadTransaction, Seeker, WriteTransaction};

/// The tree of the object layer's own records: the stored schema and the
/// auto-increment counters.
pub(crate) const META: &str = "\0meta";

/// The key of the stored schema in [`META`].
pub(crate) const SCHEMA_KEY: &[u8] = b"schema";

/// What the name of every tree of a collection's objects begins with.
pub(crate) const RECORDS_PREFIX: &str = "\0rec/";

/// What the name of every tree of an index begins with.
pub(crate) const INDEX_PREFIX: &str = "\0idx/";

/// The tree of collection `id`'s objects.
pub(crate) fn records(id: u64) -> IdName {
    IdName::new(RECORDS_PREFIX, id)
}

/// The tree of index `id`.
pub(crate) fn index_tree(id: u64) -> IdName {
    IdName::new(INDEX_PREFIX, id)
}

/// A name made of a prefix and an id, kept inline: the name of a tree of the
/// object layer, or the key of a counter in [`META`]. Every read and write of
/// an object names a tree or two, and an insert reads and stores its
/// collection's counter; a name formatted on the heap each time cost more
/// than the lookup it served.
#[derive(Clone, Copy)]
pub(crate) struct IdName {
    bytes: [u8; 32],
    len: usize,
}

impl IdName {
    fn new(prefix: &str, id: u64) -> Self {
        let mut digits = [0u8; 20];
        let mut start = digits.len();
        let mut rest = id;

        loop {
            start -= 1;
            // A remainder of a division by ten is a digit.
            digits[start] = b'0' + (rest % 10) as u8;
            rest /= 10;

            if rest == 0 {
                break;
            }
        }

        let digits = &digits[start..];
        let len = prefix.len() + digits.len();
        let mut bytes = [0u8; 32];

        bytes[..prefix.len()].copy_from_slice(prefix.as_bytes());
        bytes[prefix.len()..len].copy_from_slice(digits);

        Self { bytes, len }
    }
}

impl std::ops::Deref for IdName {
    type Target = str;

    fn deref(&self) -> &str {
        // A prefix and digits, both ASCII, so this never fails.
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

impl std::fmt::Debug for IdName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, formatter)
    }
}

/// The key of collection `id`'s next auto-increment number in [`META`].
pub(crate) fn counter(id: u64) -> IdName {
    IdName::new("next/", id)
}

/// What reading objects needs of a transaction, read or write.
pub(crate) trait Source {
    fn get_in(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Gives `visit` the value under `key`, borrowed where it lies, and
    /// returns whether there was one.
    fn get_in_with(
        &self,
        tree: &str,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool>;

    fn range_in(
        &self,
        tree: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Result<Range<'_>>;

    fn len_in(&self, tree: &str) -> Result<u64>;

    /// Lookups in `tree` of one key after another, each from where the last
    /// one ended.
    fn seeker_in(&self, tree: &str) -> Result<Seeker<'_>>;

    /// The error for damage found in the file.
    fn corrupted(&self, reason: String) -> Error;

    /// Whether the file holds the schema whose record is `encoded`.
    fn holds_schema(&self, encoded: &[u8]) -> Result<bool> {
        Ok(self.get_in(META, SCHEMA_KEY)?.as_deref() == Some(encoded))
    }
}

impl Source for ReadTransaction {
    fn get_in(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        ReadTransaction::get_in(self, tree, key)
    }

    fn get_in_with(
        &self,
        tree: &str,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        ReadTransaction::get_in_with(self, tree, key, visit)
    }

    fn range_in(
        &self,
        tree: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Result<Range<'_>> {
        ReadTransaction::range_in::<&[u8]>(self, tree, &(start, end), backward)
    }

    fn len_in(&self, tree: &str) -> Result<u64> {
        ReadTransaction::len_in(self, tree)
    }

    fn seeker_in(&self, tree: &str) -> Result<Seeker<'_>> {
        ReadTransaction::seeker_in(self, tree)
    }

    fn corrupted(&self, reason: String) -> Error {
        ReadTransaction::corrupted(self, reason)
    }

    fn holds_schema(&self, encoded: &[u8]) -> Result<bool> {
        Ok(self.get_in_kept(META, SCHEMA_KEY)?.as_deref() == Some(encoded))
    }
}

impl Source for WriteTransaction {
    fn get_in(&self, tree: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        WriteTransaction::get_in(self, tree, key)
    }

    fn get_in_with(
        &self,
        tree: &str,
        key: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        WriteTransaction::get_in_with(self, tree, key, visit)
    }

    fn range_in(
        &self,
        tree: &str,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
        backward: bool,
    ) -> Result<Range<'_>> {
        WriteTransaction::range_in::<&[u8]>(self, tree, &(start, end), backward)
    }

    fn len_in(&self, tree: &str) -> Result<u64> {
        WriteTransaction::len_in(self, tree)
    }

    fn seeker_in(&self, tree: &str) -> Result<Seeker<'_>> {
        WriteTransaction::seeker_in(self, tree)
    }

    fn corrupted(&self, reason: String) -> Error {
        WriteTransaction::corrupted(self, reason)
    }
}

/// The schema a transaction's objects are read and written with, once the
/// file is known to still hold it: another process may have migrated the
/// file since the handle opened it.
pub(crate) fn checked_schema(
    source: &dyn Source,
    schema: Option<&Arc<OpenSchema>>,
    checked: &AtomicBool,
) -> Result<Arc<OpenSchema>> {
    let schema = schema.ok_or_else(|| Error::InvalidArgument {
        message: "the database was opened without a schema, so it has no collections; declare one with `OpenOptions::schema`".to_owned(),
    })?;

    if checked.load(Ordering::Relaxed) {
        return Ok(Arc::clone(schema));
    }

    if !source.holds_schema(&schema.encoded)? {
        return Err(Error::SchemaMismatch {
            message: "another process migrated the database's schema since this handle opened it; open it again with the new schema".to_owned(),
        });
    }

    checked.store(true, Ordering::Relaxed);

    Ok(Arc::clone(schema))
}

/// The position of collection `name` in `schema`.
pub(crate) fn position(schema: &StoredSchema, name: &str) -> Result<usize> {
    schema
        .collections
        .iter()
        .position(|collection| collection.name == name)
        .ok_or_else(|| Error::InvalidArgument {
            message: format!("the schema has no collection called `{name}`"),
        })
}

/// Refuses the object of `collection` whose record is `record` and whose
/// index entries are `entries` if the record is 4 GiB long or more, or an
/// entry is longer than `max_key_len`, as [`CollectionWriter::store`] does
/// for an insert or a put. The entries of the object it replaces passed when
/// they were written.
fn check_lengths(
    collection: &CollectionDef,
    max_key_len: usize,
    record: &[u8],
    entries: &IndexKeys,
) -> Result<()> {
    if u32::try_from(record.len()).is_err() {
        return Err(Error::InvalidArgument {
            message: format!("an object of `{}` is 4 GiB long or more", collection.name),
        });
    }

    if entries.iter().any(|(_, entry)| entry.len() > max_key_len) {
        return Err(too_long(collection, max_key_len));
    }

    Ok(())
}

/// The error for an object of `collection` with a key or an index entry
/// longer than `max_key_len`.
fn too_long(collection: &CollectionDef, max_key_len: usize) -> Error {
    Error::InvalidArgument {
        message: format!(
            "an object of `{}` has a key or an indexed value too long for the file's keys, which are at most {max_key_len} bytes",
            collection.name
        ),
    }
}

/// The key encoding of `key`, once it is known to be a key of `collection`.
pub(crate) fn key_bytes(collection: &CollectionDef, key: &Value) -> Result<Vec<u8>> {
    let fits = matches!(
        (collection.key_field().map(|field| &field.kind), key),
        (Some(Kind::Int), Value::Int(_))
            | (Some(Kind::String), Value::String(_))
            | (Some(Kind::Bytes), Value::Bytes(_))
    );

    if !fits {
        return Err(Error::InvalidArgument {
            message: format!("{key:?} is not a primary key of `{}`", collection.name),
        });
    }

    key::encoded(key).map_err(internal)
}

pub(crate) fn internal(reason: &str) -> Error {
    Error::Internal {
        message: reason.to_owned(),
    }
}

/// The object of `collection` whose record is `bytes`.
pub(crate) fn decode(
    source: &dyn Source,
    collection: &CollectionDef,
    bytes: &[u8],
) -> Result<Object> {
    codec::object_of(bytes, &collection.fields)
        .map_err(|reason| damaged(source, collection, reason))
}

/// The error for an object of `collection` whose record is damaged.
fn damaged(source: &dyn Source, collection: &CollectionDef, reason: &str) -> Error {
    source.corrupted(format!("an object of `{}`: {reason}", collection.name))
}

/// Decodes objects of one collection read one after another, with the
/// fields of each put in their places by the order of the fields by name:
/// the order the caller knows, the one its schema worked out when it was
/// opened, or else one worked out at the second object for the rest, the
/// first being decoded as [`decode`] does, since working the order out for
/// one object alone would cost more than it saves.
pub(crate) struct Decoder<'a> {
    collection: &'a CollectionDef,
    known: Option<&'a codec::NameOrder>,
    worked: Option<codec::NameOrder>,
    first: bool,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(collection: &'a CollectionDef, known: Option<&'a codec::NameOrder>) -> Self {
        Self {
            collection,
            known,
            worked: None,
            first: true,
        }
    }

    pub(crate) fn decode(&mut self, source: &dyn Source, bytes: &[u8]) -> Result<Object> {
        let collection = self.collection;
        let order = match self.known {
            Some(order) => order,
            None if std::mem::take(&mut self.first) => return decode(source, collection, bytes),
            None => self
                .worked
                .get_or_insert_with(|| codec::NameOrder::of(&collection.fields)),
        };

        codec::object_in_order(bytes, &collection.fields, order)
            .map_err(|reason| damaged(source, collection, reason))
    }
}

/// The object of `collection` whose primary key is `key`, if there is one,
/// decoded from its record where the record lies, with its fields put in
/// their places by `order` when the caller knows the collection's.
pub(crate) fn get(
    source: &dyn Source,
    collection: &CollectionDef,
    order: Option<&codec::NameOrder>,
    key: &Value,
) -> Result<Option<Object>> {
    let mut object = None;

    source.get_in_with(
        &records(collection.id),
        &key_bytes(collection, key)?,
        &mut |bytes| {
            object = Some(match order {
                Some(order) => codec::object_in_order(bytes, &collection.fields, order)
                    .map_err(|reason| damaged(source, collection, reason))?,
                None => decode(source, collection, bytes)?,
            });

            Ok(())
        },
    )?;

    Ok(object)
}

/// The record of the object of `collection` whose primary key is `key`.
fn get_record(
    source: &dyn Source,
    collection: &CollectionDef,
    key: &Value,
) -> Result<Option<Vec<u8>>> {
    source.get_in(&records(collection.id), &key_bytes(collection, key)?)
}

/// Gives `visit` the record of the object of `collection` whose primary key
/// is `key`, borrowed where it lies, and returns whether there was one.
fn get_record_with(
    source: &dyn Source,
    collection: &CollectionDef,
    key: &Value,
    visit: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    source.get_in_with(&records(collection.id), &key_bytes(collection, key)?, visit)
}

/// Every object of `collection`, in primary key order or its reverse, with
/// its fields put in their places by `order`, the collection's, if the
/// caller knows it.
pub(crate) fn scan<'a>(
    source: &'a dyn Source,
    collection: &'a CollectionDef,
    order: Option<&'a codec::NameOrder>,
    backward: bool,
) -> Result<impl Iterator<Item = Result<Object>> + 'a> {
    let range = source.range_in(
        &records(collection.id),
        Bound::Unbounded,
        Bound::Unbounded,
        backward,
    )?;

    let mut decoder = Decoder::new(collection, order);

    Ok(range.map(move |entry| entry.and_then(|(_, bytes)| decoder.decode(source, &bytes))))
}

/// The entries of one object in one index: a key and a value each.
type Entries = Vec<(Vec<u8>, Vec<u8>)>;

/// The entries an object has in the indexes of its collection: the key of
/// each, one after another in one buffer, with the position of its index in
/// the collection. An entry's value is the object's primary key in a unique
/// index and empty in any other, so only the keys are kept. A vector for
/// each key and each value, and one for the entries of each index, made
/// replacing an object with two indexes cost about eighteen allocations,
/// which took a tenth of the time the replacement did.
#[derive(Debug, Default)]
struct IndexKeys {
    bytes: Vec<u8>,
    /// The position of each entry's index, and where the entry's key ends in
    /// `bytes`.
    ends: Vec<(usize, usize)>,
}

/// The bytes [`IndexKeys`] makes room for at once for each index: a tag, a
/// short value and a primary key. Making room for each entry as it came
/// reallocated the bytes for every entry after the first, twice for each
/// object an update replaced.
const ENTRY_ROOM: usize = 48;

impl IndexKeys {
    /// Room for the entries of a collection with `indexes` indexes, one each.
    fn with_capacity(indexes: usize) -> Self {
        let mut entries = Self::default();

        entries.reserve(indexes);
        entries
    }

    /// Makes room for one more entry in each of `indexes` indexes.
    fn reserve(&mut self, indexes: usize) {
        self.bytes.reserve(ENTRY_ROOM * indexes);
        self.ends.reserve(indexes);
    }

    /// Every entry: the position of its index, and its key.
    fn iter(&self) -> impl Iterator<Item = (usize, &[u8])> {
        let mut start = 0;

        self.ends.iter().map(move |&(position, end)| {
            let entry = &self.bytes[start..end];

            start = end;

            (position, entry)
        })
    }

    /// Whether index `position` has the entry `key` here.
    fn contains(&self, position: usize, key: &[u8]) -> bool {
        self.iter().any(|entry| entry == (position, key))
    }

    /// Adds an entry of index `position` whose key `encode` writes, with
    /// room for about `len` bytes.
    fn push(
        &mut self,
        position: usize,
        len: usize,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        let start = self.bytes.len();

        self.bytes.reserve(len);

        if let Err(error) = encode(&mut self.bytes) {
            self.bytes.truncate(start);

            return Err(error);
        }

        self.ends.push((position, self.bytes.len()));

        Ok(())
    }
}

/// The entries that `object`, whose primary key encodes as `key`, has in
/// `index`: a key and a value each, without repeats.
pub(crate) fn index_entries(
    index: &IndexDef,
    collection: &CollectionDef,
    object: &Object,
    key: &[u8],
) -> Result<Entries> {
    let mut entries = IndexKeys::default();

    object_entries(&mut entries, 0, index, collection, object, key)?;

    let value = if index.unique {
        key.to_vec()
    } else {
        Vec::new()
    };

    Ok(entries
        .iter()
        .map(|(_, entry)| (entry.to_vec(), value.clone()))
        .collect())
}

/// Adds the entries that `object`, whose primary key encodes as `key`, has
/// in `index`, the index at `position` in `collection`, without repeats.
fn object_entries(
    entries: &mut IndexKeys,
    position: usize,
    index: &IndexDef,
    collection: &CollectionDef,
    object: &Object,
    key: &[u8],
) -> Result<()> {
    let field = indexed_field(index, collection)?;

    found_entries(
        entries,
        position,
        index,
        field,
        object.get(&field.name),
        key,
    )
}

/// [`object_entries`] for an object whose value of the indexed field `field`
/// is `found`.
fn found_entries(
    entries: &mut IndexKeys,
    position: usize,
    index: &IndexDef,
    field: &FieldDef,
    found: Option<&Value>,
    key: &[u8],
) -> Result<()> {
    // The value the object is stored with: a required field left out holds
    // its default, as the record does.
    let value = match found {
        None | Some(Value::Null) if !field.optional => {
            field.default.as_ref().unwrap_or(&Value::Null)
        }
        Some(value) => value,
        None => &Value::Null,
    };

    value_entries(entries, position, index, value, key)
}

/// Adds the entries of `index`, at `position`, for the stored object whose
/// record is `record` and whose primary key encodes as `key`: what
/// [`object_entries`] adds for the object the record decodes to, read from
/// the one field the index is on. Replacing or deleting an object needs only
/// its entries, which decoding the whole object for cost more than the rest
/// of a change that rewrites one index.
fn stored_entries(
    entries: &mut IndexKeys,
    position: usize,
    index: &IndexDef,
    collection: &CollectionDef,
    record: &[u8],
    key: &[u8],
) -> Result<(), Unread> {
    let field = indexed_field(index, collection)?;
    let found = codec::find_field(record, field.id).map_err(Unread::Damaged)?;

    if let Some(found) = found {
        if scalar_entry(entries, position, index, found, &field.kind, key)? {
            return Ok(());
        }
    }

    let value = match found {
        Some(found) => codec::field_value(found, &field.kind).map_err(Unread::Damaged)?,
        None => match &field.default {
            Some(default) => default.clone(),
            None if field.optional => Value::Null,
            None => return Err(Unread::Damaged("a record lacks a required field")),
        },
    };

    Ok(value_entries(entries, position, index, &value, key)?)
}

/// Why the entries of a stored object were not read: what is wrong with its
/// record, for the caller to make the error for, or another failure.
enum Unread {
    Damaged(&'static str),
    Failed(Error),
}

impl From<Error> for Unread {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

/// Adds the entries in every index of `collection` of the stored object
/// whose record is `record` and whose key is `key`, for a visitor of the
/// collection's tree while the transaction changes it: what is wrong with a
/// damaged record goes to `damage`, since the transaction cannot make the
/// error then, and [`released`] makes it.
fn record_entries(
    entries: &mut IndexKeys,
    collection: &CollectionDef,
    record: &[u8],
    key: &[u8],
    damage: &mut Option<&'static str>,
) -> Result<()> {
    entries.reserve(collection.indexes.len());

    for (position, index) in collection.indexes.iter().enumerate() {
        stored_entries(entries, position, index, collection, record, key).map_err(|unread| {
            match unread {
                Unread::Damaged(reason) => {
                    *damage = Some(reason);

                    internal(reason)
                }
                Unread::Failed(error) => error,
            }
        })?;
    }

    Ok(())
}

/// The outcome of a change to a collection's tree whose visitor read a
/// record with [`record_entries`], with the error for a damaged record made
/// now that the tree is free.
fn released<T>(
    source: &dyn Source,
    collection: &CollectionDef,
    result: Result<T>,
    damage: Option<&'static str>,
) -> Result<T> {
    match (result, damage) {
        (Err(_), Some(reason)) => Err(damaged(source, collection, reason)),
        (result, _) => result,
    }
}

/// Whether the object whose entries are `entries` holds a unique value that
/// the object it replaces, whose entries are `old`, did not.
// Inlined by hand, as `btree::write::store_value` is, and for the same
// reason.
#[inline(always)]
fn adds_unique(collection: &CollectionDef, entries: &IndexKeys, old: &IndexKeys) -> bool {
    entries.iter().any(|(position, entry)| {
        collection.indexes[position].unique && !old.contains(position, entry)
    })
}

/// The field of `collection` that `index` is on.
fn indexed_field<'c>(index: &IndexDef, collection: &'c CollectionDef) -> Result<&'c FieldDef> {
    collection
        .fields
        .by_id(index.field)
        .ok_or_else(|| internal("an index is on a field its collection does not have"))
}

/// Adds the entries of `index`, at `position`, for an object whose value of
/// the indexed field is `value` and whose primary key encodes as `key`.
fn value_entries(
    entries: &mut IndexKeys,
    position: usize,
    index: &IndexDef,
    value: &Value,
    key: &[u8],
) -> Result<()> {
    // A list gives an entry for each of its values, once each and in order;
    // any other value gives one.
    let Value::List(elements) = value else {
        return entries.push(position, entry_len(index, value, key), |entry| {
            encode_entry(index, value, key, entry)
        });
    };
    let mut keys = BTreeSet::new();

    for value in elements {
        let mut entry = Vec::with_capacity(entry_len(index, value, key));

        encode_entry(index, value, key, &mut entry)?;
        keys.insert(entry);
    }

    for entry in keys {
        entries.push(position, entry.len(), |bytes| {
            bytes.extend_from_slice(&entry);

            Ok(())
        })?;
    }

    Ok(())
}

/// Adds the entry of `index`, at `position`, for an object whose indexed
/// field holds `found` in its record, what [`value_entries`] adds for the
/// value `found` reads as, encoded where the record holds it. Adds nothing
/// and returns false for anything but a scalar of the field's own kind,
/// which goes through a value: making one first cost a string for every
/// object whose string field an index is on, each time the object was
/// written, replaced or deleted.
fn scalar_entry(
    entries: &mut IndexKeys,
    position: usize,
    index: &IndexDef,
    found: FieldRef<'_>,
    kind: &Kind,
    key: &[u8],
) -> Result<bool> {
    let len = match (found, kind) {
        (FieldRef::Bool(_), Kind::Bool)
        | (FieldRef::Int(_), Kind::Int)
        | (FieldRef::Float(_), Kind::Float) => 0,
        (FieldRef::String(text), Kind::String) => text.len(),
        (FieldRef::Bytes(bytes), Kind::Bytes) => bytes.len(),
        _ => return Ok(false),
    };

    entries.push(position, len + room(index, false, key), |entry| {
        key::encode_field(found, entry).map_err(internal)?;
        name_object(index, false, key, entry);

        Ok(())
    })?;

    Ok(true)
}

/// The position in `collection`'s list of the field whose id is `id`.
fn field_position(collection: &CollectionDef, id: u64) -> Result<usize> {
    collection
        .fields
        .list
        .iter()
        .position(|field| field.id == id)
        .ok_or_else(|| internal("an index is on a field its collection does not have"))
}

/// Adds the entries of `index`, at `index_position`, for the object of a
/// collection whose fields all hold scalars and whose values an update made
/// `present`, each read as its field's already: the indexed field's value
/// where `present` holds it, or else the value the object is stored with, as
/// [`object_entries`] finds it in the object. `stored_key` is the position
/// of the key field and the key.
///
/// [`CollectionWriter::write_record`] does the same for a binding's record,
/// and keeps its own copy: calling this from there made puts through a
/// binding 2% slower, measured on separately built binaries.
fn present_entries(
    entries: &mut IndexKeys,
    (index_position, index): (usize, &IndexDef),
    collection: &CollectionDef,
    present: &[(usize, FieldRef<'_>)],
    (key_position, key_value): (usize, &Value),
    key: &[u8],
) -> Result<()> {
    let position = field_position(collection, index.field)?;
    let field = &collection.fields.list[position];
    let found = present.iter().find(|(at, _)| *at == position);

    if let Some((_, found)) = found {
        if scalar_entry(entries, index_position, index, *found, &field.kind, key)? {
            return Ok(());
        }
    }

    let value = match found {
        Some((_, found)) => codec::field_value(*found, &field.kind).map_err(internal)?,
        None if position == key_position => key_value.clone(),
        None if !field.optional => field.default.clone().unwrap_or(Value::Null),
        None => Value::Null,
    };

    value_entries(entries, index_position, index, &value, key)
}

/// About how many bytes the entry `value` gives `index` for the object whose
/// key is `key` takes.
fn entry_len(index: &IndexDef, value: &Value, key: &[u8]) -> usize {
    let len = match value {
        Value::String(text) => text.len(),
        Value::Bytes(bytes) => bytes.len(),
        _ => 0,
    };

    len + room(index, value.is_null(), key)
}

/// Writes the key of the entry `value` gives `index` for the object whose
/// key is `key`.
fn encode_entry(index: &IndexDef, value: &Value, key: &[u8], entry: &mut Vec<u8>) -> Result<()> {
    key::encode(value, entry).map_err(internal)?;
    name_object(index, value.is_null(), key, entry);

    Ok(())
}

/// The bytes an entry of `index` takes beyond its value's own: its tag and
/// length, and the object's key where the entry's key holds it.
fn room(index: &IndexDef, null: bool, key: &[u8]) -> usize {
    9 + if keyed(index, null) { key.len() } else { 0 }
}

/// Ends the key of an entry of `index` for the object whose key is `key`,
/// whose value is null when `null`.
fn name_object(index: &IndexDef, null: bool, key: &[u8], entry: &mut Vec<u8>) {
    if keyed(index, null) {
        entry.extend_from_slice(key);
    }
}

/// Whether the key of an entry of `index` ends with the object's key. A
/// unique index keys by the value alone, except for null, which any number
/// of objects may hold. Its entries name the object in their value either
/// way.
fn keyed(index: &IndexDef, null: bool) -> bool {
    !index.unique || null
}

/// The value of an entry of `index` for the object whose key is `key`.
fn entry_value<'k>(index: &IndexDef, key: &'k [u8]) -> &'k [u8] {
    if index.unique { key } else { &[] }
}

/// A collection of a read transaction: its objects as of the transaction's
/// commit.
#[derive(Debug)]
pub struct CollectionReader<'a> {
    txn: &'a ReadTransaction,
    schema: Arc<OpenSchema>,
    position: usize,
}

impl<'a> CollectionReader<'a> {
    pub(crate) fn new(txn: &'a ReadTransaction, name: &str) -> Result<Self> {
        let schema = checked_schema(txn, txn.schema(), txn.schema_checked())?;
        let position = position(&schema.schema, name)?;

        Ok(Self {
            txn,
            schema,
            position,
        })
    }

    /// The collection at `position` in `schema`, which the caller checked
    /// the file still holds.
    pub(crate) fn at(txn: &'a ReadTransaction, schema: Arc<OpenSchema>, position: usize) -> Self {
        Self {
            txn,
            schema,
            position,
        }
    }

    fn definition(&self) -> &CollectionDef {
        &self.schema.schema.collections[self.position]
    }

    /// The transaction, the schema and the collection, for running a query.
    pub(crate) fn parts(&self) -> (&dyn Source, &StoredSchema, &CollectionDef) {
        (self.txn, &self.schema.schema, self.definition())
    }

    /// The order of the collection's fields by name.
    pub(crate) fn order(&self) -> Option<&codec::NameOrder> {
        self.schema.order(self.position)
    }

    /// The object whose primary key is `key`, if there is one.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>> {
        get(
            self.txn,
            self.definition(),
            self.schema.order(self.position),
            &key.into(),
        )
    }

    /// The record of the object whose primary key is `key`, as the file holds
    /// it, for a language binding that decodes records itself; see
    /// [`get_record`](CollectionWriter::get_record).
    pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>> {
        get_record(self.txn, self.definition(), &key.into())
    }

    /// Gives `visit` the record of the object whose primary key is `key`,
    /// as [`get_record`](Self::get_record) returns it, but borrowed rather
    /// than copied into a vector of its own, and returns whether there was
    /// one: a binding that copies the record into a buffer of its own copies
    /// it once. An error `visit` returns is returned.
    pub fn get_record_with(
        &self,
        key: impl Into<Value>,
        mut visit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        get_record_with(self.txn, self.definition(), &key.into(), &mut visit)
    }

    /// Every object, in primary key order.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_> {
        scan(
            self.txn,
            self.definition(),
            self.schema.order(self.position),
            false,
        )
    }

    /// The number of objects.
    pub fn len(&self) -> Result<u64> {
        self.txn.len_in(&records(self.definition().id))
    }

    /// Whether the collection holds no object.
    pub fn is_empty(&self) -> Result<bool> {
        self.len().map(|len| len == 0)
    }
}

/// An object about to be stored: what the checks and the trees need of it.
struct Written {
    key: Vec<u8>,
    /// The primary key, which the write returns.
    key_value: Value,
    record: Vec<u8>,
    /// The object's entries in the indexes of its collection.
    entries: IndexKeys,
    /// What the write does to the auto-increment counter.
    numbering: Numbering,
}

/// What writing an object does to its collection's auto-increment counter.
#[derive(Clone, Copy)]
enum Numbering {
    /// Nothing.
    Kept,
    /// Stores this as the next number, past the one the object was given.
    Raised(u64),
    /// Raises it past the key the object brings, if the key is not below
    /// it, unless the object replaces one: storing that one raised the
    /// counter past the key already, so a replacement need not read it.
    PastKey(i64),
}

/// What a replacement stored its record over, for a refusal to undo.
enum Previous {
    /// Nothing: undoing takes the record out again.
    Nothing,
    /// An object whose unique values the new one all holds too, so that
    /// nothing after can refuse the new one.
    Object,
    /// An object whose record this is, to put back.
    Record(Vec<u8>),
}

/// What an update learns while its visitor has the record stored, for
/// [`CollectionWriter::finish_update`].
#[derive(Default)]
struct Update {
    /// The entries the object has in the indexes the update may change, and
    /// those it had, when it changes the record.
    entries: Option<(IndexKeys, IndexKeys)>,
    /// The record stored, when a unique value the object did not hold may
    /// refuse the new one after it is stored, and it has to go back.
    previous: Option<Vec<u8>>,
    /// What is wrong with the record stored, if it is damaged.
    damage: Option<&'static str>,
}

impl Update {
    /// Keeps what an update that replaces the record `stored` needs after
    /// the visitor: the entries, and the record if it may have to go back.
    fn made(
        &mut self,
        collection: &CollectionDef,
        stored: &[u8],
        entries: IndexKeys,
        old: IndexKeys,
    ) {
        if adds_unique(collection, &entries, &old) {
            self.previous = Some(stored.to_vec());
        }

        self.entries = Some((entries, old));
    }
}

/// The record of `object` of `collection`, whose fields are in `order` by
/// name and whose key encodes as `key`, and its entries in every index, as
/// [`CollectionWriter::write`] makes them for an object with its key.
fn object_written(
    schema: &OpenSchema,
    collection: &CollectionDef,
    order: &codec::NameOrder,
    object: &Object,
    key: &[u8],
) -> Result<(Vec<u8>, IndexKeys)> {
    let slots = codec::Slots::of(object, &collection.fields, order);
    let record = codec::record_of_slots(object, &slots, &collection.fields, &|id| {
        schema.schema.key_kind(id)
    })
    .map_err(|message| Error::InvalidArgument {
        message: format!("an object of `{}`: {message}", collection.name),
    })?;
    let mut entries = IndexKeys::with_capacity(collection.indexes.len());

    for (index_position, index) in collection.indexes.iter().enumerate() {
        let position = field_position(collection, index.field)?;
        let field = &collection.fields.list[position];

        found_entries(
            &mut entries,
            index_position,
            index,
            field,
            slots.get(position),
            key,
        )?;
    }

    Ok((record, entries))
}

/// What an update of an object of a collection whose fields all hold scalars
/// writes: the new record, and the entries of the indexes on the fields it
/// changes, before and after.
struct Changed {
    record: Vec<u8>,
    entries: IndexKeys,
    old: IndexKeys,
}

/// What the update `changes`, which [`codec::flat_changes`] read, makes of
/// the object of `collection` whose record is `stored` and whose key encodes
/// as `key`; `stored_key` is the position of the key field and the key.
/// `None` when the record stays as it is. What is wrong with a damaged
/// record is told apart, for the caller to make the error for once the tree
/// it reads the record from is free.
fn changed(
    collection: &CollectionDef,
    stored: &[u8],
    changes: &[(usize, Option<FieldRef<'_>>)],
    stored_key: (usize, &Value),
    key: &[u8],
) -> Result<Option<Changed>, Unread> {
    let fields = &collection.fields;
    let mut present = codec::stored_flat_fields(stored, fields).map_err(Unread::Damaged)?;

    // Every record holds the required fields without a default, and one
    // that lacks one is damaged, as reading the object finds it.
    if fields.list.iter().enumerate().any(|(position, field)| {
        !field.optional && field.default.is_none() && !present.iter().any(|(at, _)| *at == position)
    }) {
        return Err(Unread::Damaged("a record lacks a required field"));
    }

    for &(position, value) in changes {
        let at = present.iter().position(|(at, _)| *at == position);

        match (at, value) {
            (Some(at), Some(value)) => present[at].1 = value,
            (Some(at), None) => {
                present.swap_remove(at);
            }
            (None, Some(value)) => present.push((position, value)),
            (None, None) => {}
        }
    }

    // Only a change can leave out a required field now: one made null.
    let record =
        codec::flat_record(&present, fields, None).map_err(|message| Error::InvalidArgument {
            message: format!("an object of `{}`: {message}", collection.name),
        })?;

    if record == stored {
        return Ok(None);
    }

    let mut entries = IndexKeys::default();
    let mut old = IndexKeys::default();

    for (index_position, index) in collection.indexes.iter().enumerate() {
        let position = field_position(collection, index.field)?;

        if !changes.iter().any(|(at, _)| *at == position) {
            continue;
        }

        stored_entries(&mut old, index_position, index, collection, stored, key)?;
        present_entries(
            &mut entries,
            (index_position, index),
            collection,
            &present,
            stored_key,
            key,
        )?;
    }

    Ok(Some(Changed {
        record,
        entries,
        old,
    }))
}

/// Whether `found` is the primary key `key`.
fn is_key(found: FieldRef<'_>, key: &Value) -> bool {
    match (found, key) {
        (FieldRef::Int(found), Value::Int(key)) => found == *key,
        (FieldRef::String(found), Value::String(key)) => found == key.as_bytes(),
        (FieldRef::Bytes(found), Value::Bytes(key)) => found == key.as_slice(),
        _ => false,
    }
}

/// The error for an update of an object of `collection` that changes its
/// primary key.
fn changes_key(collection: &CollectionDef) -> Error {
    Error::InvalidArgument {
        message: format!(
            "an update of an object of `{}` cannot change its primary key; delete the object and insert it again",
            collection.name
        ),
    }
}

/// A collection of a write transaction: its objects, with the transaction's
/// changes, and the calls that change them.
#[derive(Debug)]
pub struct CollectionWriter<'a> {
    txn: &'a mut WriteTransaction,
    schema: Arc<OpenSchema>,
    position: usize,
}

impl<'a> CollectionWriter<'a> {
    pub(crate) fn new(txn: &'a mut WriteTransaction, name: &str) -> Result<Self> {
        let schema = checked_schema(txn, txn.schema(), txn.schema_checked())?;
        let position = position(&schema.schema, name)?;

        Ok(Self {
            txn,
            schema,
            position,
        })
    }

    /// The collection at `position` in `schema`, which the caller checked
    /// the file still holds.
    pub(crate) fn at(
        txn: &'a mut WriteTransaction,
        schema: Arc<OpenSchema>,
        position: usize,
    ) -> Self {
        Self {
            txn,
            schema,
            position,
        }
    }

    fn definition(&self) -> &CollectionDef {
        &self.schema.schema.collections[self.position]
    }

    /// The transaction, the schema and the collection, for running a query.
    pub(crate) fn parts(&self) -> (&dyn Source, &StoredSchema, &CollectionDef) {
        (&*self.txn, &self.schema.schema, self.definition())
    }

    /// The order of the collection's fields by name.
    pub(crate) fn order(&self) -> Option<&codec::NameOrder> {
        self.schema.order(self.position)
    }

    /// Inserts `object` and returns its primary key.
    ///
    /// In a collection keyed by an auto-increment, an object without an `id`
    /// gets the next number. It fails with [`Error::DuplicateKey`] if the key
    /// is taken, or if a unique index finds one of the object's values taken,
    /// and with [`Error::InvalidArgument`] if the object does not fit the
    /// schema. Either way the transaction is left as it was.
    pub fn insert(&mut self, object: Object) -> Result<Value> {
        self.write(object, false)
    }

    /// Inserts `object`, or replaces the object with its primary key, and
    /// returns the key. It fails as [`insert`](Self::insert) does, except that
    /// a taken key is not a failure.
    pub fn put(&mut self, object: Object) -> Result<Value> {
        self.write(object, true)
    }

    /// Deletes the object whose primary key is `key`, and returns whether
    /// there was one.
    pub fn delete(&mut self, key: impl Into<Value>) -> Result<bool> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];
        let key = key_bytes(collection, &key.into())?;
        let mut entries = IndexKeys::default();
        let mut damage = None;
        // The object goes first, read on its way out for its entries.
        let found = self
            .txn
            .remove_in_with(&records(collection.id), &key, &mut |stored| {
                record_entries(&mut entries, collection, stored, &key, &mut damage)
            });
        let found = released(&*self.txn, collection, found, damage)?;

        // Its entries, which it has just been read for.
        for (position, entry) in entries.iter() {
            let tree = index_tree(collection.indexes[position].id);

            self.txn.remove_present_in(&tree, entry)?;
        }

        Ok(found)
    }

    /// The object whose primary key is `key`, if there is one.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>> {
        get(
            &*self.txn,
            self.definition(),
            self.schema.order(self.position),
            &key.into(),
        )
    }

    /// The record of the object whose primary key is `key`, as the file holds
    /// it (`design/objects.md`, "Records"), for a language binding that
    /// decodes records itself.
    ///
    /// It is not checked here, so the binding treats it as untrusted: a
    /// record written before a field existed lacks that field, which reads as
    /// its default or null, and one may hold ids of fields the schema no
    /// longer has, which are skipped.
    pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>> {
        get_record(&*self.txn, self.definition(), &key.into())
    }

    /// Gives `visit` the record of the object whose primary key is `key`,
    /// with this transaction's changes; see
    /// [`CollectionReader::get_record_with`].
    pub fn get_record_with(
        &self,
        key: impl Into<Value>,
        mut visit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        get_record_with(&*self.txn, self.definition(), &key.into(), &mut visit)
    }

    /// Inserts the object whose record is `record`, as a language binding
    /// sends it: the fields it has, by id, which the write checks and fills
    /// in as [`insert`](Self::insert) does. A record that does not decode, or
    /// holds an id or a type its collection does not have, is
    /// [`Error::InvalidArgument`].
    pub fn insert_record(&mut self, record: &[u8]) -> Result<Value> {
        self.write_record(record, false)
    }

    /// Inserts or replaces the object whose record is `record`; see
    /// [`insert_record`](Self::insert_record) and [`put`](Self::put).
    pub fn put_record(&mut self, record: &[u8]) -> Result<Value> {
        self.write_record(record, true)
    }

    /// Sets the fields `changes` has in the object whose primary key is
    /// `key`, and returns whether there was one; nothing is written when
    /// there is none. The object becomes what [`put`](Self::put) would write
    /// for the object stored with those fields set: a field set to
    /// [`Value::Null`] becomes null, or its default if it is required and has
    /// one, and an embedded object or a list is replaced whole.
    ///
    /// It fails as `put` does, leaving the transaction as it was: with
    /// [`Error::DuplicateKey`] for a value of a unique field that another
    /// object holds, and with [`Error::InvalidArgument`] for a field the
    /// collection does not have, a value of another type, or a primary key
    /// other than `key`.
    pub fn update(&mut self, key: impl Into<Value>, changes: Object) -> Result<bool> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];
        let order = schema
            .order(self.position)
            .ok_or_else(|| internal("a collection has no order of its fields"))?;
        let key_value = key.into();

        if let Some(field) = collection.key_field() {
            if changes
                .get(&field.name)
                .is_some_and(|value| *value != key_value)
            {
                return Err(changes_key(collection));
            }
        }

        let key = key_bytes(collection, &key_value)?;
        let max_key_len = self.txn.max_key_len();
        let mut changes = Some(changes);
        let mut update = Update::default();
        let found = self
            .txn
            .update_in_with(&records(collection.id), &key, &mut |stored| {
                let mut object = codec::object_in_order(stored, &collection.fields, order)
                    .map_err(|reason| {
                        update.damage = Some(reason);

                        internal(reason)
                    })?;

                object.absorb(changes.take().unwrap_or_default());

                let (record, entries) = object_written(&schema, collection, order, &object, &key)?;

                if record == stored {
                    return Ok(None);
                }

                check_lengths(collection, max_key_len, &record, &entries)?;

                let mut old = IndexKeys::with_capacity(collection.indexes.len());

                record_entries(&mut old, collection, stored, &key, &mut update.damage)?;
                update.made(collection, stored, entries, old);

                Ok(Some(record))
            });

        self.finish_update(collection, &key, found, update)
    }

    /// Ends an update whose record [`update_in_with`] replaced, which found
    /// `found`: checks the unique values it adds, putting the record back if
    /// one is taken, and changes the entries of the indexes.
    ///
    /// [`update_in_with`]: WriteTransaction::update_in_with
    fn finish_update(
        &mut self,
        collection: &CollectionDef,
        key: &[u8],
        found: Result<bool>,
        update: Update,
    ) -> Result<bool> {
        let found = released(&*self.txn, collection, found, update.damage)?;
        let Some((entries, old)) = update.entries else {
            // No object, or one the changes leave as it was.
            return Ok(found);
        };

        if let Err(error) = self.check_unique(collection, key, &entries, &old) {
            let previous = update.previous.map_or(Previous::Object, Previous::Record);

            self.put_back(collection, key, previous)?;

            return Err(error);
        }

        // Nothing below can refuse the change.
        self.replace_entries(collection, key, &entries, &old)?;

        Ok(true)
    }

    /// Sets the fields of the object whose primary key is `key` that the
    /// record `changes` holds, as a language binding sends them: by id, as
    /// [`insert_record`](Self::insert_record) takes a record, a field to be
    /// made null holding the tag `0x01` (`design/objects.md`, "Records").
    /// See [`update`](Self::update).
    ///
    /// In a collection whose fields all hold scalars, the record stored is
    /// changed where it lies, and only the indexes on the fields changed are
    /// read: decoding the object and writing it whole again cost as much as
    /// the rest of the update. Any other collection's goes through objects.
    pub fn update_record(&mut self, key: impl Into<Value>, changes: &[u8]) -> Result<bool> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];
        let key_value = key.into();
        let refused = |reason: &str| Error::InvalidArgument {
            message: format!(
                "the changes to an object of `{}`: {reason}",
                collection.name
            ),
        };

        if !codec::is_flat(&collection.fields) {
            let changes = codec::changes_object_of(changes, &collection.fields).map_err(refused)?;

            return self.update(key_value, changes);
        }

        let key = key_bytes(collection, &key_value)?;
        let changes = codec::flat_changes(changes, &collection.fields).map_err(refused)?;
        let key_position = field_position(collection, collection.key)?;

        if changes.iter().any(|&(position, value)| {
            position == key_position && !value.is_some_and(|value| is_key(value, &key_value))
        }) {
            return Err(changes_key(collection));
        }

        // The record stored is read where it lies for the new record and
        // the entries of the indexes on the fields that change, and replaced
        // on the same way down. As for a put, a unique index checked after
        // that may refuse a value the object did not hold, and the record
        // replaced is kept aside then, to go back.
        let max_key_len = self.txn.max_key_len();
        let mut update = Update::default();
        let found = self
            .txn
            .update_in_with(&records(collection.id), &key, &mut |stored| {
                let changed = changed(
                    collection,
                    stored,
                    &changes,
                    (key_position, &key_value),
                    &key,
                )
                .map_err(|unread| match unread {
                    Unread::Damaged(reason) => {
                        update.damage = Some(reason);

                        internal(reason)
                    }
                    Unread::Failed(error) => error,
                })?;
                let Some(Changed {
                    record,
                    entries,
                    old,
                }) = changed
                else {
                    return Ok(None);
                };

                check_lengths(collection, max_key_len, &record, &entries)?;
                update.made(collection, stored, entries, old);

                Ok(Some(record))
            });

        self.finish_update(collection, &key, found, update)
    }

    /// Writes the object whose record a binding sent. In a collection whose
    /// fields all hold scalars, the record is checked where it lies and
    /// completed into the record the file holds: reading it into an object
    /// and writing that again cost a value and a name for every field, and a
    /// search by name for each. Any other collection's record goes through an
    /// object, whose embedded objects get their defaults on the way.
    fn write_record(&mut self, record: &[u8], replace: bool) -> Result<Value> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];

        if !codec::is_flat(&collection.fields) {
            let object = self.record_object(record)?;

            return self.write(object, replace);
        }

        let refused = |reason: &str| Error::InvalidArgument {
            message: format!("a record for `{}`: {reason}", collection.name),
        };
        let present = codec::flat_fields(record, &collection.fields).map_err(refused)?;
        let (key_position, key_field) = collection
            .fields
            .list
            .iter()
            .enumerate()
            .find(|(_, field)| field.id == collection.key)
            .ok_or_else(|| internal("a collection has no key field"))?;
        let given = present
            .iter()
            .find(|(position, _)| *position == key_position)
            .map(|(_, found)| codec::field_value(*found, &key_field.kind))
            .transpose()
            .map_err(refused)?;
        let (assigned, numbering) = if collection.auto {
            self.number(collection, given.as_ref())?
        } else {
            (None, Numbering::Kept)
        };
        let key_value = match assigned {
            Some(number) => Value::Int(number),
            None => given.unwrap_or(Value::Null),
        };
        let key = key_bytes(collection, &key_value)?;
        let stored = codec::flat_record(
            &present,
            &collection.fields,
            assigned.map(|number| (key_position, number)),
        )
        .map_err(|message| Error::InvalidArgument {
            message: format!("an object of `{}`: {message}", collection.name),
        })?;
        let mut entries = IndexKeys::with_capacity(collection.indexes.len());

        for (index_position, index) in collection.indexes.iter().enumerate() {
            let (position, field) = collection
                .fields
                .list
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == index.field)
                .ok_or_else(|| internal("an index is on a field its collection does not have"))?;
            let found = present.iter().find(|(at, _)| *at == position);

            if let Some((_, found)) = found {
                if scalar_entry(
                    &mut entries,
                    index_position,
                    index,
                    *found,
                    &field.kind,
                    &key,
                )? {
                    continue;
                }
            }

            // The value the object is stored with, as `object_entries` finds
            // it in the object.
            let value = match found {
                Some((_, found)) => codec::field_value(*found, &field.kind).map_err(refused)?,
                None if position == key_position => key_value.clone(),
                None if !field.optional => field.default.clone().unwrap_or(Value::Null),
                None => Value::Null,
            };

            value_entries(&mut entries, index_position, index, &value, &key)?;
        }

        self.store(
            collection,
            Written {
                key,
                key_value,
                record: stored,
                entries,
                numbering,
            },
            replace,
        )
    }

    fn record_object(&self, record: &[u8]) -> Result<Object> {
        let collection = self.definition();

        codec::partial_object_of(record, &collection.fields).map_err(|reason| {
            Error::InvalidArgument {
                message: format!("a record for `{}`: {reason}", collection.name),
            }
        })
    }

    /// Every object, in primary key order.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_> {
        scan(
            &*self.txn,
            self.definition(),
            self.schema.order(self.position),
            false,
        )
    }

    /// The number of objects.
    pub fn len(&self) -> Result<u64> {
        self.txn.len_in(&records(self.definition().id))
    }

    /// Whether the collection holds no object.
    pub fn is_empty(&self) -> Result<bool> {
        self.len().map(|len| len == 0)
    }

    fn write(&mut self, object: Object, replace: bool) -> Result<Value> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];
        let order = schema
            .order(self.position)
            .ok_or_else(|| internal("a collection has no order of its fields"))?;
        let key_position = collection
            .fields
            .list
            .iter()
            .position(|field| field.id == collection.key)
            .ok_or_else(|| internal("a collection has no key field"))?;
        // The number an auto-increment assigns goes into the record without
        // going into the object, which would have moved its later fields.
        let assigned_value;
        let mut slots = codec::Slots::of(&object, &collection.fields, order);
        let numbering = if collection.auto {
            let (assigned, numbering) = self.number(collection, slots.get(key_position))?;

            if let Some(number) = assigned {
                assigned_value = Value::Int(number);
                slots.set(key_position, &assigned_value);
            }

            numbering
        } else {
            Numbering::Kept
        };
        let key_value = slots.get(key_position).cloned().unwrap_or(Value::Null);
        let key = key_bytes(collection, &key_value)?;
        let record = codec::record_of_slots(&object, &slots, &collection.fields, &|id| {
            schema.schema.key_kind(id)
        })
        .map_err(|message| Error::InvalidArgument {
            message: format!("an object of `{}`: {message}", collection.name),
        })?;
        let mut entries = IndexKeys::with_capacity(collection.indexes.len());

        for (position, index) in collection.indexes.iter().enumerate() {
            let (at, field) = collection
                .fields
                .list
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == index.field)
                .ok_or_else(|| internal("an index is on a field its collection does not have"))?;

            found_entries(&mut entries, position, index, field, slots.get(at), &key)?;
        }

        self.store(
            collection,
            Written {
                key,
                key_value,
                record,
                entries,
                numbering,
            },
            replace,
        )
    }

    /// Stores an object of `collection` with its index entries, after the
    /// checks that need the trees: a unique value or, for an insert, a
    /// primary key already taken. A replacement takes out the entries of the
    /// object it replaces that the new one does not have. Returns the key.
    ///
    /// A replacement stores its record first, reading the one it replaces on
    /// the way for its entries, which goes down the collection's tree once
    /// rather than twice; a unique value found taken after that puts the
    /// replaced record back. An insert finds out whether its key is taken by
    /// storing the record, after every other check.
    fn store(
        &mut self,
        collection: &CollectionDef,
        written: Written,
        replace: bool,
    ) -> Result<Value> {
        let Written {
            key,
            key_value,
            record,
            entries,
            numbering,
        } = written;
        let max_key_len = self.txn.max_key_len();
        let too_long = || Error::InvalidArgument {
            message: format!(
                "an object of `{}` has a key or an indexed value too long for the file's keys, which are at most {max_key_len} bytes",
                collection.name
            ),
        };

        if key.len() > max_key_len {
            return Err(too_long());
        }

        if u32::try_from(record.len()).is_err() {
            return Err(Error::InvalidArgument {
                message: format!("an object of `{}` is 4 GiB long or more", collection.name),
            });
        }

        // Every key the object has in an index, those the object it replaces
        // had too, which passed when they were written.
        if entries.iter().any(|(_, entry)| entry.len() > max_key_len) {
            return Err(too_long());
        }

        let mut old = IndexKeys::default();
        let previous = if replace {
            Some(self.replace_record(collection, &key, &record, &entries, &mut old)?)
        } else {
            None
        };
        let replaced = matches!(previous, Some(Previous::Object | Previous::Record(_)));
        let checked = self
            .raised(collection, numbering, replaced)
            .and_then(|raised| {
                self.check_unique(collection, &key, &entries, &old)?;

                Ok(raised)
            });
        let raised = match checked {
            Ok(raised) => raised,
            Err(error) => {
                if let Some(previous) = previous {
                    self.put_back(collection, &key, previous)?;
                }

                return Err(error);
            }
        };

        // The last refusal: a primary key already taken, which an insert
        // learns from storing the record, and which stores nothing then.
        if !replace
            && !self
                .txn
                .insert_new_in(&records(collection.id), &key, &record)?
        {
            return Err(Error::DuplicateKey {
                message: format!(
                    "`{}` already holds an object with the primary key {key_value:?}",
                    collection.name
                ),
            });
        }

        // Nothing below can refuse the object; only a failure of the file can
        // stop it now, and that leaves the transaction unable to commit.
        // The entries of the object replaced that the object lacks, which
        // it has just been read for.
        for (position, entry) in old.iter() {
            if !entries.contains(position, entry) {
                let tree = index_tree(collection.indexes[position].id);

                self.txn.remove_present_in(&tree, entry)?;
            }
        }

        for (position, entry) in entries.iter() {
            if !old.contains(position, entry) {
                let index = &collection.indexes[position];

                self.txn
                    .insert_in(&index_tree(index.id), entry, entry_value(index, &key))?;
            }
        }

        // The counter is stored once, when the transaction commits, however
        // many objects it numbers.
        if let Some(next) = raised {
            self.txn
                .insert_later(META, counter(collection.id).as_bytes(), &next.to_le_bytes())?;
        }

        Ok(key_value)
    }

    /// Takes out the entries `old` of the object whose key is `key` that its
    /// new entries `entries` lack, and adds those it did not have, as
    /// [`store`](Self::store) does for a put. `store` keeps its own copy of
    /// this and of [`check_lengths`]: calling them from there made inserts
    /// through a binding 2% slower, measured on separately built binaries.
    fn replace_entries(
        &mut self,
        collection: &CollectionDef,
        key: &[u8],
        entries: &IndexKeys,
        old: &IndexKeys,
    ) -> Result<()> {
        for (position, entry) in old.iter() {
            if !entries.contains(position, entry) {
                let tree = index_tree(collection.indexes[position].id);

                self.txn.remove_present_in(&tree, entry)?;
            }
        }

        for (position, entry) in entries.iter() {
            if !old.contains(position, entry) {
                let index = &collection.indexes[position];

                self.txn
                    .insert_in(&index_tree(index.id), entry, entry_value(index, key))?;
            }
        }

        Ok(())
    }

    /// Stores `record` under `key`, replacing the object stored there, if
    /// any, whose entries it adds to `old`. The object whose entries are
    /// `entries` may be refused after this, for a unique value it did not
    /// hold before, so the record it replaces is kept then, to be put back.
    fn replace_record(
        &mut self,
        collection: &CollectionDef,
        key: &[u8],
        record: &[u8],
        entries: &IndexKeys,
        old: &mut IndexKeys,
    ) -> Result<Previous> {
        let mut damage = None;
        let mut previous = Previous::Nothing;
        let result = self
            .txn
            .insert_in_with(&records(collection.id), key, record, &mut |stored| {
                record_entries(old, collection, stored, key, &mut damage)?;
                previous = if adds_unique(collection, entries, old) {
                    Previous::Record(stored.to_vec())
                } else {
                    Previous::Object
                };

                Ok(())
            });

        released(&*self.txn, collection, result, damage)?;

        Ok(previous)
    }

    /// Undoes [`replace_record`](Self::replace_record) after a refusal.
    fn put_back(
        &mut self,
        collection: &CollectionDef,
        key: &[u8],
        previous: Previous,
    ) -> Result<()> {
        let tree = records(collection.id);

        match previous {
            Previous::Nothing => self.txn.remove_present_in(&tree, key).map(drop),
            Previous::Record(record) => self.txn.insert_in(&tree, key, &record),
            // Only a unique value the object did not hold can refuse it after
            // its record is stored, and the record replaced is kept then.
            Previous::Object => Err(internal(
                "a replacement was refused after nothing could refuse it",
            )),
        }
    }

    /// The counter to store, if the write moves it, where `replaced` says
    /// whether the object replaces one.
    fn raised(
        &self,
        collection: &CollectionDef,
        numbering: Numbering,
        replaced: bool,
    ) -> Result<Option<u64>> {
        Ok(match numbering {
            Numbering::Kept => None,
            Numbering::Raised(next) => Some(next),
            Numbering::PastKey(_) if replaced => None,
            Numbering::PastKey(chosen) => match u64::try_from(chosen) {
                Ok(chosen) => {
                    (chosen >= self.next_number(collection)?).then(|| chosen.saturating_add(1))
                }
                Err(_) => None,
            },
        })
    }

    /// Refuses the object whose key is `key` and whose entries are `entries`
    /// if another object holds one of the unique values it adds: the values
    /// the object it replaces, whose entries are `old`, did not hold.
    // Inlined by hand, as `btree::write::store_value` is, and for the same
    // reason.
    #[inline(always)]
    fn check_unique(
        &self,
        collection: &CollectionDef,
        key: &[u8],
        entries: &IndexKeys,
        old: &IndexKeys,
    ) -> Result<()> {
        for (position, entry) in entries.iter() {
            let index = &collection.indexes[position];

            if !index.unique || old.contains(position, entry) {
                continue;
            }

            let mut taken = false;

            self.txn
                .get_in_with(&index_tree(index.id), entry, &mut |holder| {
                    taken = holder != key;

                    Ok(())
                })?;

            if taken {
                let field = collection
                    .fields
                    .by_id(index.field)
                    .map_or("", |field| field.name.as_str());

                return Err(Error::DuplicateKey {
                    message: format!(
                        "another object of `{}` holds this value of its unique field `{field}`",
                        collection.name
                    ),
                });
            }
        }

        Ok(())
    }

    /// The collection's next auto-increment number, if the object gives no
    /// key, `given`, and what the write does to the counter.
    fn number(
        &self,
        collection: &CollectionDef,
        given: Option<&Value>,
    ) -> Result<(Option<i64>, Numbering)> {
        match given {
            None | Some(Value::Null) => {
                let next = self.next_number(collection)?;
                let assigned = i64::try_from(next).map_err(|_| Error::InvalidArgument {
                    message: format!("`{}` has used every auto-increment number", collection.name),
                })?;

                Ok((Some(assigned), Numbering::Raised(next + 1)))
            }
            Some(Value::Int(chosen)) => Ok((None, Numbering::PastKey(*chosen))),
            // Not an int: the key check refuses the object.
            Some(_) => Ok((None, Numbering::Kept)),
        }
    }

    /// The collection's next auto-increment number, with this
    /// transaction's changes.
    fn next_number(&self, collection: &CollectionDef) -> Result<u64> {
        let mut next = 1;

        self.txn
            .get_in_with(META, counter(collection.id).as_bytes(), &mut |bytes| {
                next = u64::from_le_bytes(bytes.try_into().map_err(|_| {
                    self.txn.corrupted(format!(
                        "the auto-increment counter of `{}` is not 8 bytes long",
                        collection.name
                    ))
                })?);

                Ok(())
            })?;

        Ok(next)
    }
}

/// Whether every index of `schema` holds exactly the entries its collection's
/// objects give it, for the engine's tests. The error says what differs.
#[cfg(test)]
pub(crate) fn check_indexes(source: &dyn Source, schema: &StoredSchema) -> Result<(), String> {
    for collection in &schema.collections {
        let mut objects = Vec::new();

        for entry in source
            .range_in(
                &records(collection.id),
                Bound::Unbounded,
                Bound::Unbounded,
                false,
            )
            .map_err(|error| error.to_string())?
        {
            let (key, bytes) = entry.map_err(|error| error.to_string())?;
            let object = decode(source, collection, &bytes).map_err(|error| error.to_string())?;

            objects.push((key, object));
        }

        for index in &collection.indexes {
            let mut expected = BTreeSet::new();

            for (key, object) in &objects {
                expected.extend(
                    index_entries(index, collection, object, key)
                        .map_err(|error| error.to_string())?,
                );
            }

            let actual = source
                .range_in(
                    &index_tree(index.id),
                    Bound::Unbounded,
                    Bound::Unbounded,
                    false,
                )
                .map_err(|error| error.to_string())?
                .collect::<Result<BTreeSet<_>>>()
                .map_err(|error| error.to_string())?;

            if actual != expected {
                return Err(format!(
                    "index {} of `{}` holds {} entries, {} of them wrong, where its objects give {}",
                    index.id,
                    collection.name,
                    actual.len(),
                    actual.difference(&expected).count(),
                    expected.len()
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{index_tree, records};

    #[test]
    fn a_tree_name_is_its_prefix_and_its_id_in_decimal() {
        for id in [0, 7, 10, 99, 4096, u64::MAX] {
            assert_eq!(&*records(id), format!("\0rec/{id}"));
            assert_eq!(&*index_tree(id), format!("\0idx/{id}"));
        }
    }
}
