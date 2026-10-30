//! Opening a file with a declared schema: storing it the first time, checking
//! it every time, and migrating the file from an older version
//! (`design/objects.md`, "Migrations"). And opening one without: the handle
//! takes the schema the file holds, as it is.
//!
//! A migration is one write transaction: the new stored schema, the indexes
//! it builds and drops, the application's migration functions and the
//! collections it deletes commit together or not at all.

use std::collections::VecDeque;
use std::fmt;
use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;

use super::declare::{Migration, MigrationFn, Schema};
use super::objects::{self, CollectionWriter, Source, index_entries, internal};
use super::resolve::{Resolution, resolve};
use super::typed::{CollectionType, TypedWriter};
use crate::error::{Error, Result};
use crate::format::object::key;
use crate::format::object::names::{META, SCHEMA_KEY, counter, index_tree, records};
use crate::format::object::schema::{
    Fields, IndexDef, Kind, OBJECT_FORMAT, OLDEST_OBJECT_FORMAT, OpenSchema, StoredSchema,
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

/// What opening a file with a declared schema leads to.
pub(crate) enum Opened {
    /// The file holds the declared schema, stored just now if it had none.
    Ready(Arc<OpenSchema>),
    /// A migration in its write transaction, waiting for its version steps
    /// to run and for its commit. Boxed: a write transaction is large.
    Migrating(Box<Pending>),
}

/// Stores or checks the schema of the file behind `shared`, or begins a
/// migration from an older one.
///
/// A schema that needs a higher object format than the file's is refused
/// unless `upgrade` allows it, as [`OpenOptions::upgrade_format`] does the
/// file format: builds before that object format could not open the file.
///
/// [`OpenOptions::upgrade_format`]: crate::OpenOptions::upgrade_format
pub(crate) fn open(
    shared: &Arc<Shared>,
    declared: &Schema,
    migrations: &[Migration],
    upgrade: bool,
) -> Result<Opened> {
    // Most opens find the declared schema stored, which a read transaction
    // shows without waiting for the writer lock.
    {
        let read = ReadTransaction::begin(shared, None)?;

        if let Some(encoded) = read.get_in(META, SCHEMA_KEY)? {
            let schema = decode(&shared.path, &read, &encoded)?;

            if let Resolution::Unchanged = resolve(declared, Some(&schema), migrations)? {
                return Ok(Opened::Ready(Arc::new(OpenSchema::new(schema, encoded))));
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
        Resolution::Change(plan) => *plan,
        Resolution::Unchanged => {
            let (Some(schema), Some(encoded)) = (stored, encoded) else {
                return Err(internal("an unchanged schema that is not stored"));
            };

            return Ok(Opened::Ready(Arc::new(OpenSchema::new(schema, encoded))));
        }
    };
    let format = stored
        .as_ref()
        .map_or(OLDEST_OBJECT_FORMAT, |stored| stored.format);

    if plan.to.format > format && !upgrade {
        return Err(Error::InvalidArgument {
            message: format!(
                "the schema needs object format {}, a date field for one, and the file is in object format {format}, which `upgrade_format(false)` keeps it in so that older builds still open it; open it with `upgrade_format(true)` to raise it",
                plan.to.format
            ),
        });
    }
    let encoded = plan.to.encode();
    let schema = Arc::new(OpenSchema::new(plan.to, encoded));

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

    let Some(previous) = plan.from else {
        txn.commit()?;

        return Ok(Opened::Ready(schema));
    };
    let mut functions = plan.functions.into_iter().peekable();
    let steps = (previous.version + 1..=schema.schema.version)
        .map(|version| {
            let function = functions
                .next_if(|(step, _)| *step == version)
                .map(|(_, function)| function);

            (version, function)
        })
        .collect();

    Ok(Opened::Migrating(Box::new(Pending {
        txn,
        schema,
        lenient: lenient(&previous),
        previous,
        steps,
        delete: plan.delete,
    })))
}

/// The schema the file behind `shared` holds, for a handle that declared
/// none, read once as the handle opens.
///
/// `None` when the file holds no schema, or one that does not read. Opening
/// goes ahead either way, since the integrity check and the other tools work
/// without one and are what a damaged file needs; reaching a collection says
/// why there is none ([`without_schema`]). Nothing is written: a handle
/// without a declared schema never stores, checks or migrates one.
pub(crate) fn stored(shared: &Arc<Shared>) -> Option<Arc<OpenSchema>> {
    let read = ReadTransaction::begin(shared, None).ok()?;
    let encoded = read.get_in(META, SCHEMA_KEY).ok()??;
    let schema = StoredSchema::decode(&encoded).ok()?;

    // A schema the declared form cannot say, which the engine never stores,
    // reads as damaged: `Database::stored_schema` gives it in that form.
    Schema::from_stored(&schema).ok()?;

    Some(Arc::new(OpenSchema::new(schema, encoded)))
}

/// Why a transaction of a handle without a schema has no collections, as the
/// error that reaching one fails with: the file holds no schema, the one it
/// holds does not read, or it stored one after the handle opened it.
pub(crate) fn without_schema(source: &dyn Source) -> Error {
    let encoded = match source.get_in(META, SCHEMA_KEY) {
        Ok(Some(encoded)) => encoded,
        Ok(None) => {
            return Error::InvalidArgument {
                message: "the database was opened without a schema, and the file holds none, so it has no collections; declare one with `OpenOptions::schema`".to_owned(),
            };
        }
        Err(error) => return error,
    };
    let stored = match decode(source.path(), source, &encoded) {
        Ok(stored) => stored,
        Err(error) => return error,
    };

    match Schema::from_stored(&stored) {
        Err(error) => source.corrupted(format!("the stored schema: {error}")),
        Ok(_) => Error::InvalidArgument {
            message: "the file held no schema when this handle opened it; open it again to use the schema it holds now".to_owned(),
        },
    }
}

/// A migration in its write transaction, with the version steps still to
/// run. The stored schema is the new one already, and the indexes are
/// built; the functions run step by step, and the collections the steps
/// delete go at the end, before the commit.
pub(crate) struct Pending {
    txn: WriteTransaction,
    schema: Arc<OpenSchema>,
    previous: StoredSchema,
    /// `previous` with every field optional, for reading objects the
    /// functions may have written already.
    lenient: StoredSchema,
    /// The version steps to run, each with its function if it has one.
    steps: VecDeque<(u64, Option<MigrationFn>)>,
    delete: Vec<u64>,
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pending")
            .field("previous_version", &self.previous.version)
            .field("version", &self.schema.schema.version)
            .field(
                "steps",
                &self
                    .steps
                    .iter()
                    .map(|(version, _)| *version)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Pending {
    pub(crate) fn previous_version(&self) -> u64 {
        self.previous.version
    }

    pub(crate) fn previous_record(&self) -> Vec<u8> {
        self.previous.encode()
    }

    pub(crate) fn schema_record(&self) -> &[u8] {
        &self.schema.encoded
    }

    pub(crate) fn version(&self) -> u64 {
        self.schema.schema.version
    }

    /// Runs the function of the next version step, if it has one, and
    /// returns the step's version; `None` once every step has run.
    pub(crate) fn next_step(&mut self) -> Result<Option<u64>> {
        let Some((version, function)) = self.steps.pop_front() else {
            return Ok(None);
        };

        if let Some(function) = function {
            function(&mut self.migrating())?;
        }

        Ok(Some(version))
    }

    pub(crate) fn transaction(&mut self) -> &mut WriteTransaction {
        &mut self.txn
    }

    pub(crate) fn migrating(&mut self) -> Migrating<'_> {
        Migrating {
            txn: &mut self.txn,
            previous: &self.lenient,
        }
    }

    /// Runs the steps left, deletes the collections the steps delete, and
    /// commits the migration.
    pub(crate) fn finish(mut self) -> Result<Arc<OpenSchema>> {
        while self.next_step()?.is_some() {}

        // Last, so that the functions can still read what they delete.
        for collection in &self.delete {
            delete_collection(&mut self.txn, &self.previous, *collection)?;
        }

        self.txn.commit()?;

        Ok(self.schema)
    }
}

/// The stored schema whose record is `encoded`.
fn decode(path: &Path, source: &dyn Source, encoded: &[u8]) -> Result<StoredSchema> {
    StoredSchema::decode(encoded).map_err(|reason| match reason {
        Some(reason) => source.corrupted(format!("the stored schema: {reason}")),
        None => unsupported_object_format(path, encoded),
    })
}

/// The error for the stored schema `encoded`, in an object format this
/// build does not read.
pub(crate) fn unsupported_object_format(path: &Path, encoded: &[u8]) -> Error {
    Error::UnsupportedObjectFormat {
        path: path.to_path_buf(),
        found: StoredSchema::format_of(encoded)
            .and_then(|format| u32::try_from(format).ok())
            .unwrap_or(u32::MAX),
        supported: u32::try_from(OBJECT_FORMAT).unwrap_or(u32::MAX),
    }
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
                let fields = objects::index_text(index, definition);

                return Err(Error::DuplicateKey {
                    message: format!(
                        "two objects of `{}` hold the same value of `{fields}`, which the new unique index does not allow",
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

    txn.remove_in(META, counter(id).as_bytes())?;

    Ok(())
}

/// `schema` with every field optional, so that an object a migration function
/// has already written, which lacks the fields the new schema dropped, still
/// reads under it.
fn lenient(schema: &StoredSchema) -> StoredSchema {
    fn loosen(fields: &mut Fields) {
        for field in &mut fields.list {
            field.optional = true;

            match &mut field.kind {
                Kind::Object(embedded) => loosen(embedded),
                Kind::List(element) => {
                    if let Kind::Object(embedded) = element.as_mut() {
                        loosen(embedded);
                    }
                }
                _ => {}
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
    previous: &'a StoredSchema,
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

    /// The collection of `T` in the new schema; see
    /// [`WriteTransaction::collection_of`].
    pub fn collection_of<T: CollectionType>(&mut self) -> Result<TypedWriter<'_, T>> {
        self.txn.collection_of()
    }

    /// The primary keys of every object of collection `collection`, named as
    /// the schema before the migration named it, in key order.
    pub fn previous_keys(&self, collection: &str) -> Result<Vec<Value>> {
        let definition = &self.previous.collections[objects::position(self.previous, collection)?];

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
    /// it: afterwards, the fields the migration dropped read as a record
    /// without them does, as their defaults or null.
    pub fn previous(&self, collection: &str, key: impl Into<Value>) -> Result<Option<Object>> {
        let definition = &self.previous.collections[objects::position(self.previous, collection)?];

        objects::get(
            &*self.txn,
            definition,
            &records(definition.id),
            None,
            &key.into(),
        )
    }

    /// The record of the object [`previous`](Self::previous) reads, as the
    /// file holds it, for a language binding that decodes records itself
    /// with [`PendingMigration::previous_schema_record`]. A field the record
    /// lacks reads as its default or null, required or not.
    ///
    /// [`PendingMigration::previous_schema_record`]: crate::PendingMigration::previous_schema_record
    pub fn previous_record(
        &self,
        collection: &str,
        key: impl Into<Value>,
    ) -> Result<Option<Vec<u8>>> {
        let definition = &self.previous.collections[objects::position(self.previous, collection)?];

        self.txn.get_in(
            &records(definition.id),
            &objects::key_bytes(definition, &key.into())?,
        )
    }
}
