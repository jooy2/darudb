//! Changing the objects a query finds: deleting them, or setting fields in
//! each, in one call (`design/objects.md`, "Changing the objects a query
//! finds").
//!
//! The keys come first, gathered through the query's plan, and the objects
//! are changed one after another by key, since a walk borrows the
//! transaction that a change needs for itself. An update by query is all or
//! nothing: when one object refuses its change, the ones changed before it
//! get their records and index entries back, so the transaction holds what
//! it held before the call.

use std::fmt;

use super::{Query, found_keys};
use crate::error::{Error, Result};
use crate::format::object::codec;
use crate::format::object::{Object, Value};
use crate::schema::{Change, CollectionType, CollectionWriter, FieldOf, FieldType, TypedWriter};

impl CollectionWriter<'_> {
    /// Deletes every object `query` finds, after its sort, offset and limit,
    /// and returns how many there were.
    ///
    /// The keys are gathered first and the objects deleted one after
    /// another, so a query may test the fields it deletes by. Nothing can
    /// refuse a delete; a failure part-way, of the file or its reading,
    /// leaves the transaction to be aborted, as any such failure does.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`] for a query that does not fit the schema, and
    /// what reading or writing the file meets.
    pub fn delete_where(&mut self, query: &Query) -> Result<u64> {
        let (source, schema, collection) = self.parts();
        let keys = found_keys(source, schema, collection, query)?;

        for key in &keys {
            self.delete(key.clone())?;
        }

        Ok(keys.len() as u64)
    }

    /// Sets the fields `changes` has in every object `query` finds, after its
    /// sort, offset and limit, as [`update`](Self::update) sets them in one,
    /// and returns how many objects the query found.
    ///
    /// All or nothing: when an object refuses its change, by a unique value
    /// another object holds or an index entry too long, the objects changed
    /// before it get their records and index entries back, and the
    /// transaction holds what it held before the call. The changes cannot
    /// set the primary key.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`] for a query that does not fit the schema, and
    /// what [`update`](Self::update) fails with.
    pub fn update_where(&mut self, query: &Query, changes: Object) -> Result<u64> {
        let changed = changes.fields().map(|(name, _)| name.to_owned()).collect();

        self.change_where(query, changed, |writer, key| {
            writer.update(key, changes.clone())
        })
    }

    /// [`update_where`](Self::update_where) with the changes in a record, as
    /// a language binding sends them to
    /// [`update_record`](Self::update_record).
    ///
    /// # Errors
    ///
    /// As [`update_where`](Self::update_where) fails, and with
    /// [`Error::InvalidArgument`] for changes that do not decode.
    pub fn update_records_where(&mut self, query: &Query, changes: &[u8]) -> Result<u64> {
        let (_, _, collection) = self.parts();
        let changed = codec::changes_object_of(changes, &collection.fields)
            .map_err(|reason| Error::InvalidArgument {
                message: format!(
                    "the changes to an object of `{}`: {reason}",
                    collection.name
                ),
            })?
            .fields()
            .map(|(name, _)| name.to_owned())
            .collect();

        self.change_where(query, changed, |writer, key| {
            writer.update_record(key, changes)
        })
    }

    /// Runs `change` on the object with each key `query` finds, where
    /// `changed` names the fields it sets, and undoes the changes made
    /// before a refused one.
    fn change_where(
        &mut self,
        query: &Query,
        changed: Vec<String>,
        mut change: impl FnMut(&mut Self, Value) -> Result<bool>,
    ) -> Result<u64> {
        let (source, schema, collection) = self.parts();

        if collection
            .key_field()
            .is_some_and(|key| changed.contains(&key.name))
        {
            return Err(Error::InvalidArgument {
                message: format!(
                    "an update of the objects of `{}` a query finds cannot set their primary key",
                    collection.name
                ),
            });
        }

        // Only an index on a field the changes set can refuse one of them,
        // by a unique value taken or an entry too long. Without one, no
        // record is kept to undo with.
        let may_refuse = collection.indexes.iter().any(|index| {
            index.components.iter().any(|path| {
                path.first()
                    .and_then(|id| collection.fields.by_id(*id))
                    .is_some_and(|field| changed.contains(&field.name))
            })
        });
        let keys = found_keys(source, schema, collection, query)?;
        let mut undo: Vec<(Value, Vec<u8>)> = Vec::new();

        for key in &keys {
            if may_refuse {
                if let Some(record) = self.get_record(key.clone())? {
                    undo.push((key.clone(), record));
                }
            }

            if let Err(error) = change(self, key.clone()) {
                // The refused object is as it was; the ones before it go
                // back, the last first, each to a state that was there.
                if may_refuse {
                    undo.pop();
                }

                while let Some((key, record)) = undo.pop() {
                    self.restore(&key, &record)?;
                }

                return Err(error);
            }
        }

        Ok(keys.len() as u64)
    }
}

impl<'a, T: CollectionType> TypedWriter<'a, T> {
    /// Deletes every object `query` finds, and returns how many there were;
    /// see [`CollectionWriter::delete_where`].
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::delete_where`] fails.
    pub fn delete_where(&mut self, query: &Query) -> Result<u64> {
        self.untyped().delete_where(query)
    }

    /// Sets the fields `changes` has in every object `query` finds, and
    /// returns how many there were; see [`CollectionWriter::update_where`].
    /// [`change_where`](Self::change_where) names the fields through `T`
    /// instead.
    ///
    /// # Errors
    ///
    /// As [`CollectionWriter::update_where`] fails.
    pub fn update_where(&mut self, query: &Query, changes: Object) -> Result<u64> {
        self.untyped().update_where(query, changes)
    }

    /// The changes to every object `query` finds, which
    /// [`ChangeWhere::set`] makes one field at a time and
    /// [`ChangeWhere::apply`] writes together, all or nothing:
    /// `people.change_where(&adults).set(Person::ACTIVE, true).apply()`.
    pub fn change_where(&mut self, query: &Query) -> ChangeWhere<'_, 'a, T> {
        ChangeWhere {
            change: self.unkeyed_change(),
            query: query.clone(),
        }
    }
}

/// Changes to every object a query finds in a [`TypedWriter`]'s collection,
/// made one field at a time with [`set`](Self::set) and written by
/// [`apply`](Self::apply). [`TypedWriter::change_where`] makes it.
#[must_use = "the changes are written only by `apply`"]
pub struct ChangeWhere<'w, 'a, T> {
    change: Change<'w, 'a, T>,
    query: Query,
}

impl<T> fmt::Debug for ChangeWhere<'_, '_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangeWhere")
            .field("change", &self.change)
            .finish_non_exhaustive()
    }
}

impl<T: CollectionType> ChangeWhere<'_, '_, T> {
    /// Sets `field` to `value` in each object, in place of any value set for
    /// it before; see [`Change::set`].
    pub fn set<V: FieldType>(mut self, field: FieldOf<T, V>, value: V) -> Self {
        self.change = self.change.set(field, value);
        self
    }

    /// Writes the changes to every object the query finds, all or nothing,
    /// and returns how many there were; see
    /// [`CollectionWriter::update_where`].
    ///
    /// # Errors
    ///
    /// What [`set`](Self::set) met writing a value, or what
    /// [`CollectionWriter::update_records_where`] returns.
    pub fn apply(self) -> Result<u64> {
        let (writer, changes) = self.change.into_changes()?;

        writer.untyped().update_records_where(&self.query, &changes)
    }
}
