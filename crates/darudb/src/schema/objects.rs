//! Reading and writing the objects of a collection, and keeping its indexes
//! in step with them (`design/objects.md`, "Storage", "Indexes" and "Writing
//! objects").
//!
//! Every write checks everything that can refuse it, the object's types, its
//! key, the unique indexes and the lengths of every key it adds, before it
//! changes a tree. A refused write leaves the transaction as it was and able
//! to commit.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::format::object::codec;
use crate::format::object::key;
use crate::format::object::schema::{
    CollectionDef, FieldDef, IndexDef, Kind, OpenSchema, StoredSchema,
};
use crate::format::object::{Object, Value};
use crate::txn::{Range, ReadTransaction, WriteTransaction};

/// The tree of the object layer's own records: the stored schema and the
/// auto-increment counters.
pub(crate) const META: &str = "\0meta";

/// The key of the stored schema in [`META`].
pub(crate) const SCHEMA_KEY: &[u8] = b"schema";

/// The tree of collection `id`'s objects.
pub(crate) fn records(id: u64) -> IdName {
    IdName::new("\0rec/", id)
}

/// The tree of index `id`.
pub(crate) fn index_tree(id: u64) -> IdName {
    IdName::new("\0idx/", id)
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

/// The entries that `object`, whose primary key encodes as `key`, has in
/// `index`: a key and a value each, without repeats.
pub(crate) fn index_entries(
    index: &IndexDef,
    collection: &CollectionDef,
    object: &Object,
    key: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let field = indexed_field(index, collection)?;
    // The value the object is stored with: a required field left out holds
    // its default, as the record does.
    let value = match object.get(&field.name) {
        None | Some(Value::Null) if !field.optional => {
            field.default.as_ref().unwrap_or(&Value::Null)
        }
        Some(value) => value,
        None => &Value::Null,
    };

    entries_of(index, value, key)
}

/// The entries of `index` for the stored object whose record is `record`
/// and whose primary key encodes as `key`: what [`index_entries`] gives for
/// the object the record decodes to, read from the one field the index is
/// on. Replacing or deleting an object needs only its entries, which
/// decoding the whole object for cost more than the rest of a change that
/// rewrites one index.
fn stored_index_entries(
    source: &dyn Source,
    index: &IndexDef,
    collection: &CollectionDef,
    record: &[u8],
    key: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let field = indexed_field(index, collection)?;
    let value = match codec::find_field(record, field.id)
        .map_err(|reason| damaged(source, collection, reason))?
    {
        Some(found) => codec::field_value(found, &field.kind)
            .map_err(|reason| damaged(source, collection, reason))?,
        None => match &field.default {
            Some(default) => default.clone(),
            None if field.optional => Value::Null,
            None => {
                return Err(damaged(
                    source,
                    collection,
                    "a record lacks a required field",
                ));
            }
        },
    };

    entries_of(index, &value, key)
}

/// The field of `collection` that `index` is on.
fn indexed_field<'c>(index: &IndexDef, collection: &'c CollectionDef) -> Result<&'c FieldDef> {
    collection
        .fields
        .by_id(index.field)
        .ok_or_else(|| internal("an index is on a field its collection does not have"))
}

/// The entries of `index` for an object whose value of the indexed field is
/// `value` and whose primary key encodes as `key`.
fn entries_of(index: &IndexDef, value: &Value, key: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    // A list gives an entry for each of its values, once each and in order;
    // any other value gives one.
    let Value::List(elements) = value else {
        return Ok(vec![index_entry(index, value, key)?]);
    };
    let mut entries = BTreeSet::new();

    for value in elements {
        entries.insert(index_entry(index, value, key)?);
    }

    Ok(entries.into_iter().collect())
}

/// The entry `value` gives index `index` for the object whose key is `key`.
fn index_entry(index: &IndexDef, value: &Value, key: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    // A unique index keys by the value alone, except for null, which any
    // number of objects may hold. Its entries name the object in their value
    // either way.
    let keyed = !index.unique || value.is_null();
    let room = match value {
        Value::String(text) => text.len(),
        Value::Bytes(bytes) => bytes.len(),
        _ => 0,
    } + 9
        + if keyed { key.len() } else { 0 };
    let mut entry = Vec::with_capacity(room);

    key::encode(value, &mut entry).map_err(internal)?;

    if keyed {
        entry.extend_from_slice(key);
    }

    let named = if index.unique {
        key.to_vec()
    } else {
        Vec::new()
    };

    Ok((entry, named))
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
    /// The object's entries in each index of its collection, in their order.
    entries: Vec<Vec<(Vec<u8>, Vec<u8>)>>,
    /// The auto-increment counter to store, if the write moves it.
    raised: Option<u64>,
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
        let mut entries = Vec::new();
        let source: &dyn Source = &*self.txn;
        let found = source.get_in_with(&records(collection.id), &key, &mut |stored| {
            for index in &collection.indexes {
                for (entry, _) in stored_index_entries(source, index, collection, stored, &key)? {
                    entries.push((index_tree(index.id), entry));
                }
            }

            Ok(())
        })?;

        if !found {
            return Ok(false);
        }

        // The object and its entries, which it has just been read for.
        for (tree, entry) in entries {
            self.txn.remove_present_in(&tree, &entry)?;
        }

        self.txn.remove_present_in(&records(collection.id), &key)
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
        let (assigned, raised) = if collection.auto {
            self.number(collection, given.as_ref())?
        } else {
            (None, None)
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
        let mut entries = Vec::with_capacity(collection.indexes.len());

        for index in &collection.indexes {
            let (position, field) = collection
                .fields
                .list
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == index.field)
                .ok_or_else(|| internal("an index is on a field its collection does not have"))?;
            // The value the object is stored with, as `index_entries` finds
            // it in the object.
            let value = match present.iter().find(|(at, _)| *at == position) {
                Some((_, found)) => codec::field_value(*found, &field.kind).map_err(refused)?,
                None if position == key_position => key_value.clone(),
                None if !field.optional => field.default.clone().unwrap_or(Value::Null),
                None => Value::Null,
            };

            entries.push(entries_of(index, &value, &key)?);
        }

        self.store(
            collection,
            Written {
                key,
                key_value,
                record: stored,
                entries,
                raised,
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

    fn write(&mut self, mut object: Object, replace: bool) -> Result<Value> {
        let schema = Arc::clone(&self.schema);
        let collection = &schema.schema.collections[self.position];
        let key_field = collection
            .key_field()
            .ok_or_else(|| internal("a collection has no key field"))?;
        let raised = if collection.auto {
            let (assigned, raised) = self.number(collection, object.get(&key_field.name))?;

            if let Some(number) = assigned {
                object.set_named(&key_field.name, Value::Int(number));
            }

            raised
        } else {
            None
        };
        let key_value = object.get(&key_field.name).cloned().unwrap_or(Value::Null);
        let key = key_bytes(collection, &key_value)?;
        let record = codec::record_of(&object, &collection.fields, &|id| {
            schema.schema.key_kind(id)
        })
        .map_err(|message| Error::InvalidArgument {
            message: format!("an object of `{}`: {message}", collection.name),
        })?;
        let entries = collection
            .indexes
            .iter()
            .map(|index| index_entries(index, collection, &object, &key))
            .collect::<Result<_>>()?;

        self.store(
            collection,
            Written {
                key,
                key_value,
                record,
                entries,
                raised,
            },
            replace,
        )
    }

    /// Stores an object of `collection` with its index entries, after the
    /// checks that need the trees: a unique value or, for an insert, a
    /// primary key already taken. A replacement takes out the entries of the
    /// object it replaces that the new one does not have. Returns the key.
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
            raised,
        } = written;

        // An insert finds out whether the key is taken by storing the record,
        // below, rather than by looking it up first. A replacement reads the
        // entries the object it replaces has in each index.
        let mut old: Vec<Vec<(Vec<u8>, Vec<u8>)>> = Vec::new();

        if replace {
            let source: &dyn Source = &*self.txn;

            source.get_in_with(&records(collection.id), &key, &mut |stored| {
                for index in &collection.indexes {
                    old.push(stored_index_entries(
                        source, index, collection, stored, &key,
                    )?);
                }

                Ok(())
            })?;
        }

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

        let mut removals = Vec::new();
        let mut additions = Vec::with_capacity(collection.indexes.len());

        for ((position, index), after) in collection.indexes.iter().enumerate().zip(entries) {
            let tree = index_tree(index.id);
            let before = old.get(position).map_or(&[][..], Vec::as_slice);

            for entry in before {
                if !after.contains(entry) {
                    removals.push((tree, entry.0.clone()));
                }
            }

            for entry in after {
                if before.contains(&entry) {
                    continue;
                }

                if entry.0.len() > max_key_len {
                    return Err(too_long());
                }

                if index.unique {
                    let mut taken = false;

                    self.txn.get_in_with(&tree, &entry.0, &mut |holder| {
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

                additions.push((tree, entry));
            }
        }

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
        // The entries of the object replaced, which it has just been read
        // for.
        for (tree, entry) in removals {
            self.txn.remove_present_in(&tree, &entry)?;
        }

        for (tree, (entry, value)) in additions {
            self.txn.insert_in(&tree, &entry, &value)?;
        }

        // The counter is stored once, when the transaction commits, however
        // many objects it numbers.
        if let Some(next) = raised {
            self.txn
                .insert_later(META, counter(collection.id).as_bytes(), &next.to_le_bytes())?;
        }

        if replace {
            self.txn.insert_in(&records(collection.id), &key, &record)?;
        }

        Ok(key_value)
    }

    /// The collection's next auto-increment number, if the object gives no
    /// key, `given`, and the next number to store if it changes: past the
    /// one assigned, or past a key the object brings that is not below it.
    fn number(
        &self,
        collection: &CollectionDef,
        given: Option<&Value>,
    ) -> Result<(Option<i64>, Option<u64>)> {
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

        match given {
            None | Some(Value::Null) => {
                let assigned = i64::try_from(next).map_err(|_| Error::InvalidArgument {
                    message: format!("`{}` has used every auto-increment number", collection.name),
                })?;

                Ok((Some(assigned), Some(next + 1)))
            }
            Some(Value::Int(chosen)) => Ok((
                None,
                u64::try_from(*chosen)
                    .ok()
                    .filter(|chosen| *chosen >= next)
                    .map(|chosen| chosen.saturating_add(1)),
            )),
            // Not an int: the key check refuses the object.
            Some(_) => Ok((None, None)),
        }
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
