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
use crate::format::object::schema::{CollectionDef, IndexDef, Kind, OpenSchema, StoredSchema};
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

    if source.get_in(META, SCHEMA_KEY)?.as_deref() != Some(schema.encoded.as_slice()) {
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
        .map_err(|reason| source.corrupted(format!("an object of `{}`: {reason}", collection.name)))
}

/// Decodes objects of one collection read one after another: the first as
/// [`decode`] does, and from the second on with the order of the fields by
/// name worked out once for the rest, which working out for one object alone
/// would cost more than it saves.
pub(crate) struct Decoder<'a> {
    collection: &'a CollectionDef,
    order: Option<codec::NameOrder>,
    first: bool,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(collection: &'a CollectionDef) -> Self {
        Self {
            collection,
            order: None,
            first: true,
        }
    }

    pub(crate) fn decode(&mut self, source: &dyn Source, bytes: &[u8]) -> Result<Object> {
        let collection = self.collection;

        if std::mem::take(&mut self.first) {
            return decode(source, collection, bytes);
        }

        let order = self
            .order
            .get_or_insert_with(|| codec::NameOrder::of(&collection.fields));

        codec::object_in_order(bytes, &collection.fields, order).map_err(|reason| {
            source.corrupted(format!("an object of `{}`: {reason}", collection.name))
        })
    }
}

/// The object of `collection` whose primary key is `key`, if there is one,
/// decoded from its record where the record lies.
pub(crate) fn get(
    source: &dyn Source,
    collection: &CollectionDef,
    key: &Value,
) -> Result<Option<Object>> {
    let mut object = None;

    source.get_in_with(
        &records(collection.id),
        &key_bytes(collection, key)?,
        &mut |bytes| {
            object = Some(decode(source, collection, bytes)?);

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

/// Every object of `collection`, in primary key order or its reverse.
pub(crate) fn scan<'a>(
    source: &'a dyn Source,
    collection: &'a CollectionDef,
    backward: bool,
) -> Result<impl Iterator<Item = Result<Object>> + 'a> {
    let range = source.range_in(
        &records(collection.id),
        Bound::Unbounded,
        Bound::Unbounded,
        backward,
    )?;

    let mut decoder = Decoder::new(collection);

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
    let field = collection
        .fields
        .by_id(index.field)
        .ok_or_else(|| internal("an index is on a field its collection does not have"))?;
    // The value the object is stored with: a required field left out holds
    // its default, as the record does.
    let value = match object.get(&field.name) {
        None | Some(Value::Null) if !field.optional => {
            field.default.as_ref().unwrap_or(&Value::Null)
        }
        Some(value) => value,
        None => &Value::Null,
    };
    let values = match value {
        Value::List(elements) => elements.iter().collect(),
        value => vec![value],
    };
    let mut entries = BTreeSet::new();

    for value in values {
        let mut entry = key::encoded(value).map_err(internal)?;

        // A unique index keys by the value alone, except for null, which any
        // number of objects may hold. Its entries name the object in their
        // value either way.
        if !index.unique || value.is_null() {
            entry.extend_from_slice(key);
        }

        let named = if index.unique {
            key.to_vec()
        } else {
            Vec::new()
        };

        entries.insert((entry, named));
    }

    Ok(entries.into_iter().collect())
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

    /// The object whose primary key is `key`, if there is one.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>> {
        get(self.txn, self.definition(), &key.into())
    }

    /// The record of the object whose primary key is `key`, as the file holds
    /// it, for a language binding that decodes records itself; see
    /// [`get_record`](CollectionWriter::get_record).
    pub fn get_record(&self, key: impl Into<Value>) -> Result<Option<Vec<u8>>> {
        get_record(self.txn, self.definition(), &key.into())
    }

    /// Every object, in primary key order.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_> {
        scan(self.txn, self.definition(), false)
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
        let Some(bytes) = self.txn.get_in(&records(collection.id), &key)? else {
            return Ok(false);
        };
        let old = decode(self.txn, collection, &bytes)?;

        for index in &collection.indexes {
            for (entry, _) in index_entries(index, collection, &old, &key)? {
                self.txn.remove_in(&index_tree(index.id), &entry)?;
            }
        }

        self.txn.remove_in(&records(collection.id), &key)
    }

    /// The object whose primary key is `key`, if there is one.
    pub fn get(&self, key: impl Into<Value>) -> Result<Option<Object>> {
        get(&*self.txn, self.definition(), &key.into())
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

    /// Inserts the object whose record is `record`, as a language binding
    /// sends it: the fields it has, by id, which the write checks and fills
    /// in as [`insert`](Self::insert) does. A record that does not decode, or
    /// holds an id or a type its collection does not have, is
    /// [`Error::InvalidArgument`].
    pub fn insert_record(&mut self, record: &[u8]) -> Result<Value> {
        let object = self.record_object(record)?;

        self.write(object, false)
    }

    /// Inserts or replaces the object whose record is `record`; see
    /// [`insert_record`](Self::insert_record) and [`put`](Self::put).
    pub fn put_record(&mut self, record: &[u8]) -> Result<Value> {
        let object = self.record_object(record)?;

        self.write(object, true)
    }

    fn record_object(&self, record: &[u8]) -> Result<Object> {
        let collection = self.definition();

        codec::read(record)
            .and_then(|raw| codec::to_partial_object(raw, &collection.fields))
            .map_err(|reason| Error::InvalidArgument {
                message: format!("a record for `{}`: {reason}", collection.name),
            })
    }

    /// Every object, in primary key order.
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<Object>> + '_> {
        scan(&*self.txn, self.definition(), false)
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
            self.number(collection, &mut object, &key_field.name)?
        } else {
            None
        };
        let key = key_bytes(
            collection,
            object.get(&key_field.name).unwrap_or(&Value::Null),
        )?;
        let record = codec::record_of(&object, &collection.fields, &|id| {
            schema.schema.key_kind(id)
        })
        .map_err(|message| Error::InvalidArgument {
            message: format!("an object of `{}`: {message}", collection.name),
        })?;

        // An insert finds out whether the key is taken by storing the record,
        // below, rather than by looking it up first.
        let old = if replace {
            match self.txn.get_in(&records(collection.id), &key)? {
                Some(bytes) => Some(decode(self.txn, collection, &bytes)?),
                None => None,
            }
        } else {
            None
        };
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
        let mut additions = Vec::new();

        for index in &collection.indexes {
            let tree = index_tree(index.id);
            let before = match &old {
                Some(old) => index_entries(index, collection, old, &key)?,
                None => Vec::new(),
            };
            let after = index_entries(index, collection, &object, &key)?;

            for entry in &before {
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
                    if let Some(holder) = self.txn.get_in(&tree, &entry.0)? {
                        if holder != key {
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
                    "`{}` already holds an object with the primary key {:?}",
                    collection.name,
                    object.get(&key_field.name).unwrap_or(&Value::Null)
                ),
            });
        }

        // Nothing below can refuse the object; only a failure of the file can
        // stop it now, and that leaves the transaction unable to commit.
        for (tree, entry) in removals {
            self.txn.remove_in(&tree, &entry)?;
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

        Ok(object.get(&key_field.name).cloned().unwrap_or(Value::Null))
    }

    /// Gives `object` the collection's next auto-increment number if it has
    /// no key, and returns the next number to store if it changes: past the
    /// one given, or past a key the object brings that is not below it.
    fn number(
        &self,
        collection: &CollectionDef,
        object: &mut Object,
        name: &str,
    ) -> Result<Option<u64>> {
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

        match object.get(name) {
            None | Some(Value::Null) => {
                let assigned = i64::try_from(next).map_err(|_| Error::InvalidArgument {
                    message: format!("`{}` has used every auto-increment number", collection.name),
                })?;

                object.set_named(name, Value::Int(assigned));

                Ok(Some(next + 1))
            }
            Some(Value::Int(chosen)) => Ok(u64::try_from(*chosen)
                .ok()
                .filter(|chosen| *chosen >= next)
                .map(|chosen| chosen.saturating_add(1))),
            // Not an int: the key check refuses the object.
            Some(_) => Ok(None),
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
