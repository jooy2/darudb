//! What an application declares: the schema, built at run time, and the
//! migrations from one version to the next.

use std::fmt;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::schema::{FieldDef, Kind, MAX_COMPONENTS, StoredSchema, path_fields};

use super::typed::{CollectionType, EmbeddedType};

/// The type of a field.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Type {
    /// `false` or `true`.
    Bool,
    /// A signed 64-bit integer.
    Int,
    /// A 64-bit floating-point number.
    Float,
    /// UTF-8 text.
    String,
    /// Any bytes.
    Bytes,
    /// A link to an object of the named collection: its primary key.
    Link(String),
    /// A list of values of a scalar type or links.
    List(Box<Type>),
    /// An embedded object with fields of its own.
    Object(Embedded),
    /// An instant, in microseconds since the Unix epoch: [`Value::Date`].
    /// A file with a date field is in object format 2, which builds before
    /// it cannot open.
    Date,
}

impl Type {
    /// A link to an object of `collection`.
    pub fn link(collection: impl Into<String>) -> Self {
        Type::Link(collection.into())
    }

    /// A list of values of `element`, which is a scalar type or a link.
    pub fn list(element: Type) -> Self {
        Type::List(Box::new(element))
    }

    /// An embedded object with the fields of `embedded`.
    pub fn object(embedded: Embedded) -> Self {
        Type::Object(embedded)
    }
}

/// A field of a collection or of an embedded object, as a schema declares it.
///
/// The builders of [`Collection`] and [`Embedded`] make fields; reading one
/// back is for a program that looks at a schema it did not declare, such as
/// the one [`Database::stored_schema`](crate::Database::stored_schema) gives
/// for a file opened without a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub(crate) name: String,
    pub(crate) kind: Type,
    pub(crate) optional: bool,
    pub(crate) default: Option<Value>,
}

impl Field {
    /// The field's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The field's type.
    pub fn kind(&self) -> &Type {
        &self.kind
    }

    /// Whether the field may be null, which it is when an object leaves it
    /// out.
    pub fn is_optional(&self) -> bool {
        self.optional
    }

    /// The value an object that leaves the field out holds, for a required
    /// field that has one.
    pub fn default(&self) -> Option<&Value> {
        self.default.as_ref()
    }
}

/// The fields of an embedded object.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Embedded {
    pub(crate) fields: Vec<Field>,
}

impl Embedded {
    /// The fields, in the order they were declared, or in the file's order
    /// for a schema read from a file.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The fields `E` declares, as `#[derive(Embedded)]` declares them.
    pub fn of<E: EmbeddedType>() -> Self {
        E::embedded()
    }

    /// An embedded object with no field yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a required field.
    #[must_use]
    pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self {
        self.fields.push(field(name, kind, false, None));
        self
    }

    /// Adds an optional field, which may be null.
    #[must_use]
    pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self {
        self.fields.push(field(name, kind, true, None));
        self
    }

    /// Adds a required field that holds `value` when it is left out.
    #[must_use]
    pub fn with_default(
        mut self,
        name: impl Into<String>,
        kind: Type,
        value: impl Into<Value>,
    ) -> Self {
        self.fields
            .push(field(name, kind, false, Some(value.into())));
        self
    }
}

fn field(name: impl Into<String>, kind: Type, optional: bool, default: Option<Value>) -> Field {
    Field {
        name: name.into(),
        kind,
        optional,
        default,
    }
}

/// An index of a collection, as a schema declares it: the fields it is on,
/// in order, and whether it is unique.
///
/// [`Collection::index`] and [`Collection::unique`] declare one on a field;
/// [`Collection::index_on`] and [`Collection::unique_on`] one on several,
/// whose entries sort by the first, then the second and so on, or on a field
/// inside an embedded object, named by its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// Each component's path: the names from the collection down to the
    /// field.
    pub(crate) components: Vec<Vec<String>>,
    pub(crate) unique: bool,
}

impl Index {
    /// The fields the index is on, in order, each a name, or names joined by
    /// `.` through embedded objects, such as `address.city`.
    pub fn fields(&self) -> impl ExactSizeIterator<Item = String> + '_ {
        self.components.iter().map(|path| path.join("."))
    }

    /// Whether two objects may not hold the same values, though any number
    /// may hold a null in one of them.
    pub fn is_unique(&self) -> bool {
        self.unique
    }

    /// The components' paths, joined by `.`, for a message.
    fn text(&self) -> String {
        self.fields().collect::<Vec<_>>().join(", ")
    }
}

/// The components of an index on `paths`, each split into names at `.`.
fn components<P: Into<String>>(paths: impl IntoIterator<Item = P>) -> Vec<Vec<String>> {
    paths
        .into_iter()
        .map(|path| path.into().split('.').map(str::to_owned).collect())
        .collect()
}

/// A collection: its fields, its primary key and its indexes.
///
/// Without [`primary_key`](Self::primary_key), the collection's key is an
/// `int` field called `id` that the engine assigns in increasing order.
#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    pub(crate) name: String,
    pub(crate) fields: Vec<Field>,
    /// The name of the primary key's field, or `None` for the auto-increment.
    pub(crate) key: Option<String>,
    pub(crate) indexes: Vec<Index>,
}

impl Collection {
    /// The collection `T` declares, as `#[derive(Object)]` or a
    /// hand-written [`CollectionType`] declares it.
    pub fn of<T: CollectionType>() -> Self {
        T::collection()
    }

    /// A collection with no field yet, keyed by an auto-increment.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
            key: None,
            indexes: Vec::new(),
        }
    }

    /// Adds field `name` of type `kind`, an `int`, a `string` or `bytes`, as
    /// the primary key.
    #[must_use]
    pub fn primary_key(mut self, name: impl Into<String>, kind: Type) -> Self {
        let name = name.into();

        self.key = Some(name.clone());
        self.fields.push(field(name, kind, false, None));
        self
    }

    /// Adds a required field.
    #[must_use]
    pub fn field(mut self, name: impl Into<String>, kind: Type) -> Self {
        self.fields.push(field(name, kind, false, None));
        self
    }

    /// Adds an optional field, which may be null.
    #[must_use]
    pub fn optional(mut self, name: impl Into<String>, kind: Type) -> Self {
        self.fields.push(field(name, kind, true, None));
        self
    }

    /// Adds a required field that holds `value` when it is left out.
    #[must_use]
    pub fn with_default(
        mut self,
        name: impl Into<String>,
        kind: Type,
        value: impl Into<Value>,
    ) -> Self {
        self.fields
            .push(field(name, kind, false, Some(value.into())));
        self
    }

    /// Indexes field `name`, so that queries on it read the index.
    #[must_use]
    pub fn index(mut self, name: impl Into<String>) -> Self {
        self.indexes.push(Index {
            components: vec![vec![name.into()]],
            unique: false,
        });
        self
    }

    /// Indexes field `name` and keeps its values unique: two objects cannot
    /// hold the same value, though any number may hold null.
    #[must_use]
    pub fn unique(mut self, name: impl Into<String>) -> Self {
        self.indexes.push(Index {
            components: vec![vec![name.into()]],
            unique: true,
        });
        self
    }

    /// Indexes the fields `paths`, in order, as one index: its entries sort
    /// by the first field, then the second, and so on, so that a query
    /// testing the first fields for equality and the next one for a range,
    /// or sorting by the fields after them, reads it. Each is a field's name,
    /// or names joined by `.` through embedded objects, such as
    /// `address.city`, and there are 1 to 8 of them. A path through a list,
    /// or to one, is allowed only in an index of one field. A file with such
    /// an index, or an index of one field inside an embedded object, is in
    /// object format 2.
    #[must_use]
    pub fn index_on<P: Into<String>>(mut self, paths: impl IntoIterator<Item = P>) -> Self {
        self.indexes.push(Index {
            components: components(paths),
            unique: false,
        });
        self
    }

    /// [`index_on`](Self::index_on), keeping the fields' values unique
    /// together: two objects cannot hold the same values in all of them,
    /// though any number may hold a null in one.
    #[must_use]
    pub fn unique_on<P: Into<String>>(mut self, paths: impl IntoIterator<Item = P>) -> Self {
        self.indexes.push(Index {
            components: components(paths),
            unique: true,
        });
        self
    }

    /// The collection's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The fields, in the order they were declared, or in the file's order
    /// for a schema read from a file. The primary key's field is one of them,
    /// unless the key is the auto-increment, whose `id` is not declared.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The name of the primary key's field: `id` for the auto-increment.
    pub fn key(&self) -> &str {
        self.key_name()
    }

    /// Whether the primary key is the auto-increment: an `int` field called
    /// `id`, which the engine assigns in increasing order.
    pub fn is_auto_increment(&self) -> bool {
        self.key.is_none()
    }

    /// The collection's indexes, in the order they were declared, or in the
    /// file's order for a schema read from a file.
    pub fn indexes(&self) -> &[Index] {
        &self.indexes
    }

    /// The fields, the auto-increment key included.
    pub(crate) fn all_fields(&self) -> Vec<Field> {
        let mut fields = Vec::with_capacity(self.fields.len() + 1);

        if self.key.is_none() {
            fields.push(field(AUTO_KEY, Type::Int, false, None));
        }

        fields.extend(self.fields.iter().cloned());
        fields
    }

    /// The name of the primary key's field.
    pub(crate) fn key_name(&self) -> &str {
        self.key.as_deref().unwrap_or(AUTO_KEY)
    }
}

/// The name of the field an auto-increment key lives in.
pub(crate) const AUTO_KEY: &str = "id";

/// The collections of a database and what their objects hold, at one
/// version.
///
/// ```
/// use darudb::{Collection, Schema, Type};
///
/// let schema = Schema::new(1)
///     .collection(
///         Collection::new("users")
///             .field("name", Type::String)
///             .optional("email", Type::String)
///             .unique("email"),
///     )
///     .collection(
///         Collection::new("posts")
///             .field("title", Type::String)
///             .field("author", Type::link("users"))
///             .index("author"),
///     );
/// # let _ = schema;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub(crate) version: u64,
    pub(crate) collections: Vec<Collection>,
}

impl Schema {
    /// A schema at `version`, from 1 up, with no collection yet. An
    /// application raises the version whenever it changes the schema.
    pub fn new(version: u64) -> Self {
        Self {
            version,
            collections: Vec::new(),
        }
    }

    /// Adds `collection`.
    #[must_use]
    pub fn collection(mut self, collection: Collection) -> Self {
        self.collections.push(collection);
        self
    }

    /// The schema's version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The collections, in the order they were declared, or in the file's
    /// order for a schema read from a file.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    /// Reads a schema that another language declared, encoded the way the
    /// file stores a schema (`design/objects.md`, "The stored schema"), with
    /// ids of the encoder's choosing: a language binding builds its schema
    /// this way. The ids only tie links and indexes to what they name; the
    /// file gives the collections and fields ids of its own.
    ///
    /// A record that does not decode, or whose ids are inconsistent, is
    /// [`Error::InvalidArgument`]; the schema is checked like any other when
    /// the file is opened with it.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let stored = StoredSchema::decode(bytes).map_err(|reason| Error::InvalidArgument {
            message: format!(
                "a declared schema does not decode: {}",
                reason.unwrap_or("it is in an object format this build does not read")
            ),
        })?;

        Self::from_stored(&stored)
    }

    /// The schema `stored` is, as an application would declare it: the
    /// names in place of the ids. It fails with [`Error::InvalidArgument`]
    /// for what a declaration cannot say, such as an auto-increment key that
    /// is not an `int` called `id`, which the engine never stores.
    pub(crate) fn from_stored(stored: &StoredSchema) -> Result<Self> {
        let mut collections = Vec::new();

        for definition in &stored.collections {
            let key = definition
                .key_field()
                .ok_or_else(|| Error::InvalidArgument {
                    message: format!("`{}` has no primary key field", definition.name),
                })?;

            if definition.auto && (key.name != AUTO_KEY || key.kind != Kind::Int) {
                return invalid(format!(
                    "`{}` has an auto-increment key, which is an int called `{AUTO_KEY}`",
                    definition.name
                ));
            }

            let fields = definition
                .fields
                .list
                .iter()
                .filter(|field| !(definition.auto && field.id == definition.key))
                .map(|field| declared_field(field, stored))
                .collect::<Result<_>>()?;
            let indexes = definition
                .indexes
                .iter()
                .map(|index| {
                    let components = index
                        .components
                        .iter()
                        .map(|path| {
                            path_fields(&definition.fields, path)
                                .map(|steps| steps.iter().map(|step| step.name.clone()).collect())
                                .ok_or_else(|| Error::InvalidArgument {
                                    message: format!(
                                        "`{}` indexes a field it lacks",
                                        definition.name
                                    ),
                                })
                        })
                        .collect::<Result<_>>()?;

                    Ok(Index {
                        components,
                        unique: index.unique,
                    })
                })
                .collect::<Result<_>>()?;

            collections.push(Collection {
                name: definition.name.clone(),
                fields,
                key: (!definition.auto).then(|| key.name.clone()),
                indexes,
            });
        }

        Ok(Self {
            version: stored.version,
            collections,
        })
    }

    pub(crate) fn find(&self, name: &str) -> Option<&Collection> {
        self.collections
            .iter()
            .find(|collection| collection.name == name)
    }

    /// Refuses a schema that cannot be stored: see each rule below.
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version == 0 {
            return invalid("a schema's version starts at 1".to_owned());
        }

        for (index, collection) in self.collections.iter().enumerate() {
            let name = &collection.name;

            if name.is_empty() {
                return invalid("a collection's name is empty".to_owned());
            }

            if self.collections[..index]
                .iter()
                .any(|other| other.name == *name)
            {
                return invalid(format!("the collection `{name}` is declared twice"));
            }

            if collection.key.is_none()
                && collection.fields.iter().any(|field| field.name == AUTO_KEY)
            {
                return invalid(format!(
                    "`{name}` has a field called `{AUTO_KEY}` but no primary key: name it with `primary_key`, since `{AUTO_KEY}` is the auto-increment's"
                ));
            }

            let fields = collection.all_fields();

            self.validate_fields(name, &fields)?;

            let key = fields
                .iter()
                .find(|field| field.name == collection.key_name())
                .ok_or_else(|| Error::InvalidArgument {
                    message: format!("`{name}` has no primary key field"),
                })?;

            if !matches!(key.kind, Type::Int | Type::String | Type::Bytes) {
                return invalid(format!(
                    "the primary key of `{name}` is not an int, a string or bytes"
                ));
            }

            for (position, index) in collection.indexes.iter().enumerate() {
                let count = index.components.len();

                if count == 0 || count > MAX_COMPONENTS {
                    return invalid(format!(
                        "`{name}` has an index on {count} fields; an index is on 1 to {MAX_COMPONENTS}"
                    ));
                }

                for path in &index.components {
                    self.validate_component(name, &fields, path, count)?;
                }

                if collection.indexes[..position]
                    .iter()
                    .any(|other| other.components == index.components)
                {
                    return invalid(format!("`{name}` indexes `{}` twice", index.text()));
                }
            }
        }

        Ok(())
    }

    /// Refuses a component of an index of `count` components along `path`
    /// from `fields`, the fields of `owner`, that cannot be one: a name that
    /// is no field, a path past a scalar or through a link, an embedded object
    /// at its end, more than one list, or a list in an index of more than
    /// one component.
    fn validate_component(
        &self,
        owner: &str,
        fields: &[Field],
        path: &[String],
        count: usize,
    ) -> Result<()> {
        let text = path.join(".");
        let mut fields = fields;
        let mut lists = 0;

        for (at, name) in path.iter().enumerate() {
            let Some(field) = fields.iter().find(|field| field.name == *name) else {
                return invalid(format!(
                    "`{owner}` indexes `{text}`, which it does not have"
                ));
            };
            let last = at + 1 == path.len();
            let kind = match &field.kind {
                Type::List(element) => {
                    lists += 1;
                    element.as_ref()
                }
                kind => kind,
            };

            match kind {
                Type::Object(embedded) if !last => fields = &embedded.fields,
                Type::Object(_) => {
                    return invalid(format!(
                        "`{owner}.{text}` cannot be indexed: it is an embedded object or a list of them; index one of its fields"
                    ));
                }
                _ if !last => {
                    return invalid(format!(
                        "`{owner}` indexes `{text}`, which goes on past `{name}`: a path goes through embedded objects, and no link"
                    ));
                }
                _ => {}
            }
        }

        if lists > 1 || (lists == 1 && count > 1) {
            return invalid(format!(
                "`{owner}` indexes `{text}`, a path through a list, which only an index of that one field can be, through one list"
            ));
        }

        Ok(())
    }

    fn validate_fields(&self, owner: &str, fields: &[Field]) -> Result<()> {
        for (index, field) in fields.iter().enumerate() {
            let name = &field.name;

            if name.is_empty() {
                return invalid(format!("a field of `{owner}` has an empty name"));
            }

            if fields[..index].iter().any(|other| other.name == *name) {
                return invalid(format!("`{owner}` declares the field `{name}` twice"));
            }

            self.validate_type(owner, name, &field.kind)?;

            if let Some(default) = &field.default {
                if !default_fits(default, &field.kind) {
                    return invalid(format!(
                        "the default of `{owner}.{name}` does not have the field's type"
                    ));
                }
            }
        }

        Ok(())
    }

    fn validate_type(&self, owner: &str, name: &str, kind: &Type) -> Result<()> {
        match kind {
            Type::Link(target) if self.find(target).is_none() => invalid(format!(
                "`{owner}.{name}` links to `{target}`, which is not a collection"
            )),
            Type::List(element) if matches!(**element, Type::List(_)) => invalid(format!(
                "`{owner}.{name}` is a list of lists, which a schema does not have; a list of embedded objects holding lists does"
            )),
            Type::List(element) => self.validate_type(owner, name, element),
            Type::Object(embedded) => {
                self.validate_fields(&format!("{owner}.{name}"), &embedded.fields)
            }
            _ => Ok(()),
        }
    }
}

/// A field as declared, from a field of a declared schema's record.
fn declared_field(field: &FieldDef, stored: &StoredSchema) -> Result<Field> {
    Ok(Field {
        name: field.name.clone(),
        kind: declared_type(&field.kind, stored)?,
        optional: field.optional,
        default: field.default.clone(),
    })
}

fn declared_type(kind: &Kind, stored: &StoredSchema) -> Result<Type> {
    Ok(match kind {
        Kind::Bool => Type::Bool,
        Kind::Int => Type::Int,
        Kind::Float => Type::Float,
        Kind::String => Type::String,
        Kind::Bytes => Type::Bytes,
        Kind::Date => Type::Date,
        Kind::Link { collection } => Type::Link(
            stored
                .collection_by_id(*collection)
                .map(|target| target.name.clone())
                .ok_or_else(|| Error::InvalidArgument {
                    message: "a declared link names no collection".to_owned(),
                })?,
        ),
        Kind::List(element) => Type::List(Box::new(declared_type(element, stored)?)),
        Kind::Object(fields) => Type::Object(Embedded {
            fields: fields
                .list
                .iter()
                .map(|field| declared_field(field, stored))
                .collect::<Result<_>>()?,
        }),
    })
}

/// Whether `value` can be the default of a field of `kind`: a scalar of that
/// type, or a list of them. A link's default would name an object, and an
/// embedded object has its fields' defaults instead.
fn default_fits(value: &Value, kind: &Type) -> bool {
    match (kind, value) {
        (Type::Bool, Value::Bool(_))
        | (Type::Int, Value::Int(_))
        | (Type::Float, Value::Float(_))
        | (Type::String, Value::String(_))
        | (Type::Bytes, Value::Bytes(_))
        | (Type::Date, Value::Date(_)) => true,
        (Type::List(element), Value::List(values)) => {
            values.iter().all(|value| default_fits(value, element))
        }
        _ => false,
    }
}

fn invalid<T>(message: String) -> Result<T> {
    Err(Error::InvalidArgument { message })
}

/// The function a migration runs, with the write transaction that migrates.
pub(crate) type MigrationFn =
    Arc<dyn Fn(&mut crate::schema::Migrating<'_>) -> Result<()> + Send + Sync>;

/// What an application's schema version `n` changes from version `n − 1`,
/// beyond what the engine does by itself ([`Schema`] has the rules).
///
/// ```
/// use darudb::Migration;
///
/// let migration = Migration::to(2)
///     .rename_field("users", "fullname", "name")
///     .run(|migrating| {
///         // Read and write objects under the new schema here.
///         Ok(())
///     });
/// # let _ = migration;
/// ```
#[derive(Clone)]
pub struct Migration {
    pub(crate) version: u64,
    pub(crate) renamed_collections: Vec<(String, String)>,
    pub(crate) renamed_fields: Vec<(String, String, String)>,
    pub(crate) deleted_collections: Vec<String>,
    pub(crate) replaced_fields: Vec<(String, String)>,
    pub(crate) run: Option<MigrationFn>,
}

impl fmt::Debug for Migration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Migration")
            .field("version", &self.version)
            .field("renamed_collections", &self.renamed_collections)
            .field("renamed_fields", &self.renamed_fields)
            .field("deleted_collections", &self.deleted_collections)
            .field("replaced_fields", &self.replaced_fields)
            .field("run", &self.run.is_some())
            .finish()
    }
}

impl Migration {
    /// The migration to schema version `version`, from the one before.
    pub fn to(version: u64) -> Self {
        Self {
            version,
            renamed_collections: Vec::new(),
            renamed_fields: Vec::new(),
            deleted_collections: Vec::new(),
            replaced_fields: Vec::new(),
            run: None,
        }
    }

    /// Renames collection `from` to `to`. Its objects stay where they are.
    #[must_use]
    pub fn rename_collection(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.renamed_collections.push((from.into(), to.into()));
        self
    }

    /// Renames field `from` of `collection` to `to`, using the collection's
    /// name before any rename this migration makes. No object is rewritten.
    #[must_use]
    pub fn rename_field(
        mut self,
        collection: impl Into<String>,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> Self {
        self.renamed_fields
            .push((collection.into(), from.into(), to.into()));
        self
    }

    /// Deletes collection `name` and every object in it.
    #[must_use]
    pub fn delete_collection(mut self, name: impl Into<String>) -> Self {
        self.deleted_collections.push(name.into());
        self
    }

    /// Replaces field `field` of `collection` with a new field of the same
    /// name, as when its type changes. The old values stay readable in the
    /// migration's function, through [`Migrating::previous`].
    ///
    /// [`Migrating::previous`]: crate::Migrating::previous
    #[must_use]
    pub fn replace_field(
        mut self,
        collection: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        self.replaced_fields.push((collection.into(), field.into()));
        self
    }

    /// Runs `function` as part of the migration, after the schema has become
    /// the new one, in the same write transaction. An error it returns ends
    /// the migration, and the file keeps its old schema and data.
    #[must_use]
    pub fn run(
        mut self,
        function: impl Fn(&mut crate::schema::Migrating<'_>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.run = Some(Arc::new(function));
        self
    }
}
