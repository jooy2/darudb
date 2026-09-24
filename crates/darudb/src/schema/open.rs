//! Opening a file with a declared schema: storing it the first time, checking
//! it every time, and migrating the file from an older version
//! (`design/objects.md`, "Migrations").
//!
//! A migration is one write transaction: the new stored schema, the indexes
//! it builds and drops, the application's migration functions and the
//! collections it deletes commit together or not at all.

use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;

use super::declare::{Migration, Schema};
use super::objects::{
    self, CollectionWriter, META, SCHEMA_KEY, Source, counter, index_entries, index_tree, internal,
    records,
};
use super::resolve::{Resolution, resolve};
use crate::error::{Error, Result};
use crate::format::object::key;
use crate::format::object::schema::{
    Fields, IndexDef, Kind, OBJECT_FORMAT, OpenSchema, StoredSchema,
};
use crate::format::object::{Object, Value};
use crate::instance::Shared;
use crate::txn::{ReadTransaction, WriteTransaction};

/// How many objects building an index reads before it writes their entries:
/// the reads borrow the transaction the writes change.
const BUILD_BATCH: usize = 1024;

/// Refuses a declared schema or a migration that cannot be applied to any
/// file, before a file is opened or created.
pub(crate) fn check(declared: &Schema, migrations: &[Migration]) -> Result<()> {
    declared.validate()?;

    for migration in migrations {
        if migration.version < 2 || migration.version > declared.version {
            return Err(Error::InvalidArgument {
                message: format!(
                    "a migration to version {} does not lead from version 1 up to the declared version {}",
                    migration.version, declared.version
                ),
            });
        }
    }

    Ok(())
}

/// Stores, checks or migrates the schema of the file behind `shared`, and
/// returns the stored schema its handle works with.
pub(crate) fn open(
    shared: &Arc<Shared>,
    declared: &Schema,
    migrations: &[Migration],
) -> Result<Arc<OpenSchema>> {
    // Most opens find the declared schema stored, which a read transaction
    // shows without waiting for the writer lock.
    {
        let read = ReadTransaction::begin(shared, None)?;

        if let Some(encoded) = read.get_in(META, SCHEMA_KEY)? {
            let schema = decode(&shared.path, &read, &encoded)?;

            if let Resolution::Unchanged = resolve(declared, Some(&schema), migrations)? {
                return Ok(Arc::new(OpenSchema { schema, encoded }));
            }
        }
    }

    // Again under the writer lock: another process may have stored or
    // migrated the schema since, and then there is nothing left to do.
    let mut txn = WriteTransaction::begin(shared, None)?;
    let encoded = txn.get_in(META, SCHEMA_KEY)?;
    let stored = encoded
        .as_deref()
        .map(|encoded| decode(&shared.path, &txn, encoded))
        .transpose()?;
    let plan = match resolve(declared, stored.as_ref(), migrations)? {
        Resolution::Change(plan) => plan,
        Resolution::Unchanged => {
            let (Some(schema), Some(encoded)) = (stored, encoded) else {
                return Err(internal("an unchanged schema that is not stored"));
            };

            return Ok(Arc::new(OpenSchema { schema, encoded }));
        }
    };
    let schema = Arc::new(OpenSchema {
        encoded: plan.to.encode(),
        schema: plan.to,
    });

    // Stored first, so that the migration functions read and write objects
    // under the new schema like any other transaction does.
    txn.set_schema(Arc::clone(&schema));
    txn.insert_in(META, SCHEMA_KEY, &schema.encoded)?;

    for index in &plan.drop {
        txn.delete_tree_in(&index_tree(*index))?;
    }

    for (collection, index) in &plan.build {
        build_index(&mut txn, &schema.schema, *collection, index)?;
    }

    if let Some(previous) = &plan.from {
        if !plan.functions.is_empty() {
            let mut migrating = Migrating {
                txn: &mut txn,
                previous: lenient(previous),
            };

            for function in &plan.functions {
                function(&mut migrating)?;
            }
        }

        // Last, so that the functions can still read what they delete.
        for collection in &plan.delete {
            delete_collection(&mut txn, previous, *collection)?;
        }
    }

    txn.commit()?;

    Ok(schema)
}

/// The stored schema whose record is `encoded`.
fn decode(path: &Path, source: &dyn Source, encoded: &[u8]) -> Result<StoredSchema> {
    StoredSchema::decode(encoded).map_err(|reason| match reason {
        Some(reason) => source.corrupted(format!("the stored schema: {reason}")),
        None => Error::UnsupportedFormatVersion {
            path: path.to_path_buf(),
            found: StoredSchema::format_of(encoded)
                .and_then(|format| u32::try_from(format).ok())
                .unwrap_or(u32::MAX),
            supported: u32::try_from(OBJECT_FORMAT).unwrap_or(u32::MAX),
        },
    })
}

/// Fills the empty tree of `index` from the objects of `collection`,
/// failing with [`Error::DuplicateKey`] if a unique index finds a value twice.
fn build_index(
    txn: &mut WriteTransaction,
    schema: &StoredSchema,
    collection: u64,
    index: &IndexDef,
) -> Result<()> {
    let definition = schema
        .collection_by_id(collection)
        .ok_or_else(|| internal("an index to build on a collection the schema lacks"))?;
    let records = records(collection);
    let tree = index_tree(index.id);
    let max_key_len = txn.max_key_len();
    let mut after = None;

    loop {
        let mut entries = Vec::new();
        let mut read = 0;
        let start = after.as_deref().map_or(Bound::Unbounded, Bound::Excluded);

        for entry in
            Source::range_in(&*txn, &records, start, Bound::Unbounded, false)?.take(BUILD_BATCH)
        {
            let (key, bytes) = entry?;
            let object = objects::decode(&*txn, definition, &bytes)?;

            entries.extend(index_entries(index, definition, &object, &key)?);
            after = Some(key);
            read += 1;
        }

        for (entry, value) in entries {
            if entry.len() > max_key_len {
                return Err(Error::InvalidArgument {
                    message: format!(
                        "an object of `{}` has an indexed value too long for the file's keys, which are at most {max_key_len} bytes",
                        definition.name
                    ),
                });
            }

            if index.unique && txn.get_in(&tree, &entry)?.is_some() {
                let field = definition
                    .fields
                    .by_id(index.field)
                    .map_or("", |field| field.name.as_str());

                return Err(Error::DuplicateKey {
                    message: format!(
                        "two objects of `{}` hold the same value of `{field}`, which the new unique index does not allow",
                        definition.name
                    ),
                });
            }

            txn.insert_in(&tree, &entry, &value)?;
        }

        if read < BUILD_BATCH {
            return Ok(());
        }
    }
}

/// Deletes collection `id` of the `previous` schema: its objects, its
/// indexes and its auto-increment counter.
fn delete_collection(txn: &mut WriteTransaction, previous: &StoredSchema, id: u64) -> Result<()> {
    txn.delete_tree_in(&records(id))?;

    for index in previous
        .collection_by_id(id)
        .map(|collection| collection.indexes.as_slice())
        .unwrap_or_default()
    {
        txn.delete_tree_in(&index_tree(index.id))?;
    }

    txn.remove_in(META, &counter(id))?;

    Ok(())
}

/// `schema` with every field optional, so that an object a migration function
/// has already written, which lacks the fields the new schema dropped, still
/// reads under it.
fn lenient(schema: &StoredSchema) -> StoredSchema {
    fn loosen(fields: &mut Fields) {
        for field in &mut fields.list {
            field.optional = true;

            if let Kind::Object(embedded) = &mut field.kind {
                loosen(embedded);
            }
        }
    }

    let mut schema = schema.clone();

    for collection in &mut schema.collections {
        loosen(&mut collection.fields);
    }

    schema
}

/// The write transaction of a migration, as its migration functions see it.
///
/// Its collections are those of the new schema. The objects as the schema
/// before the migration read them, with the fields the migration removed or
/// replaced, stay readable through [`previous`](Self::previous) until the
/// migration commits.
#[derive(Debug)]
pub struct Migrating<'a> {
    txn: &'a mut WriteTransaction,
    /// The schema before the migration, every field of it optional.
    previous: StoredSchema,
}

impl Migrating<'_> {
    /// The schema version the file held before the migration.
    pub fn previous_version(&self) -> u64 {
        self.previous.version
    }

    /// The write transaction the migration runs in. Committing it is the
    /// engine's job, once every migration function has returned.
    pub fn transaction(&mut self) -> &mut WriteTransaction {
        self.txn
    }

    /// Collection `name` of the new schema.
    pub fn collection(&mut self, name: &str) -> Result<CollectionWriter<'_>> {
        self.txn.collection(name)
    }

    /// The primary keys of every object of collection `collection`, named as
    /// the schema before the migration named it, in key order.
    pub fn previous_keys(&self, collection: &str) -> Result<Vec<Value>> {
        let definition = &self.previous.collections[objects::position(&self.previous, collection)?];

        Source::range_in(
            &*self.txn,
            &records(definition.id),
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )?
        .map(|entry| {
            let (bytes, _) = entry?;

            match key::decode(&bytes) {
                Ok((key, used)) if used == bytes.len() => Ok(key),
                _ => Err(self.txn.corrupted(format!(
                    "a primary key of `{}` does not decode",
                    definition.name
                ))),
            }
        })
        .collect()
    }

    /// The object of collection `collection` whose primary key is `key`, as
    /// the schema before the migration reads it: with the names it gave the
    /// collection and its fields, and the values of fields the migration
    /// removed or replaced.
    ///
    /// It reads the object as it is now. Writing an object keeps only the
    /// fields of the new schema, so read an object this way before writing
    /// it: afterwards, the fields the migration dropped read as null.
    pub fn previous(&self, collection: &str, key: impl Into<Value>) -> Result<Option<Object>> {
        let definition = &self.previous.collections[objects::position(&self.previous, collection)?];

        objects::get(&*self.txn, definition, &key.into())
    }
}
