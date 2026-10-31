//! The stored schema: collections, fields and indexes with the ids the file
//! gives them, and its encoding as a record (`design/objects.md`, "The stored
//! schema").

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex, PoisonError};

use super::codec::{self, NameOrder, Raw};
use super::names::CollectionNames;
use super::value::Value;

/// The newest object layer's format this build reads and writes, field 1 of
/// the stored schema. Format 2 adds the date type, lists of embedded
/// objects, indexes on several fields or on a field inside an embedded
/// object, and cascading links.
pub(crate) const OBJECT_FORMAT: i64 = 2;

/// The oldest object format this build reads: the one every release before
/// format 2 wrote, which a schema that declares nothing newer keeps, so that
/// those releases still open the file.
pub(crate) const OLDEST_OBJECT_FORMAT: i64 = 1;

/// What a field holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Bool,
    Int,
    Float,
    String,
    Bytes,
    /// The primary key of an object in the collection with this id.
    Link {
        collection: u64,
    },
    /// Values of the element kind: a scalar, a link, or in object format 2
    /// an embedded object.
    List(Box<Kind>),
    /// An embedded object with these fields.
    Object(Fields),
    /// An instant, in microseconds since the Unix epoch. Object format 2.
    Date,
}

impl Kind {
    /// The kind in words, for an error message.
    pub(crate) fn describe(&self) -> &'static str {
        match self {
            Kind::Bool => "a bool",
            Kind::Int => "an int",
            Kind::Float => "a float",
            Kind::String => "a string",
            Kind::Bytes => "bytes",
            Kind::Link { .. } => "a link",
            Kind::List(_) => "a list",
            Kind::Object(_) => "an object",
            Kind::Date => "a date",
        }
    }

    /// Whether a primary key may have this kind.
    pub(crate) fn is_key(&self) -> bool {
        matches!(self, Kind::Int | Kind::String | Kind::Bytes)
    }

    /// Whether a value of this kind is one value a key can hold.
    pub(crate) fn is_scalar(&self) -> bool {
        matches!(
            self,
            Kind::Bool
                | Kind::Int
                | Kind::Float
                | Kind::String
                | Kind::Bytes
                | Kind::Link { .. }
                | Kind::Date
        )
    }

    fn code(&self) -> i64 {
        match self {
            Kind::Bool => 1,
            Kind::Int => 2,
            Kind::Float => 3,
            Kind::String => 4,
            Kind::Bytes => 5,
            Kind::Link { .. } => 6,
            Kind::List(_) => 7,
            Kind::Object(_) => 8,
            Kind::Date => 9,
        }
    }
}

/// One field, as the file knows it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldDef {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) optional: bool,
    pub(crate) default: Option<Value>,
    /// Whether deleting the object a link names deletes the object that
    /// holds the link. Only a to-one link of a collection itself has it.
    /// Object format 2.
    pub(crate) cascade: bool,
}

/// The fields of a collection or of an embedded object, by id in ascending
/// order, and the id the next new field gets.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Fields {
    pub(crate) list: Vec<FieldDef>,
    pub(crate) next_id: u64,
}

impl Fields {
    pub(crate) fn by_name(&self, name: &str) -> Option<&FieldDef> {
        self.list.iter().find(|field| field.name == name)
    }

    pub(crate) fn by_id(&self, id: u64) -> Option<&FieldDef> {
        self.list.iter().find(|field| field.id == id)
    }
}

/// The most components an index has.
pub(crate) const MAX_COMPONENTS: usize = 8;

/// An index of a collection: its components, in order, each the path of
/// field ids from the collection down through embedded objects to the field
/// it reads, and whether it is unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexDef {
    pub(crate) id: u64,
    pub(crate) components: Vec<Vec<u64>>,
    pub(crate) unique: bool,
}

impl IndexDef {
    /// The field of the collection itself the index is on, for an index of
    /// one component on a top-level field, which format 1 has and which
    /// keeps the fast paths of one field.
    pub(crate) fn field(&self) -> Option<u64> {
        match self.components.as_slice() {
            [path] => match path.as_slice() {
                [field] => Some(*field),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether the index is on `field`, a field of the collection itself,
    /// alone.
    pub(crate) fn is_on(&self, field: u64) -> bool {
        self.field() == Some(field)
    }
}

/// The fields a path of ids reaches, from `fields` down, or `None` where an
/// id names no field or the path goes on past a field with none: the field
/// of each step, and of the last.
pub(crate) fn path_fields<'f>(fields: &'f Fields, path: &[u64]) -> Option<Vec<&'f FieldDef>> {
    let mut found = Vec::with_capacity(path.len());
    let mut fields = fields;

    for (at, id) in path.iter().enumerate() {
        let field = fields.by_id(*id)?;

        found.push(field);

        if at + 1 < path.len() {
            fields = match &field.kind {
                Kind::Object(inner) => inner,
                Kind::List(element) => match element.as_ref() {
                    Kind::Object(inner) => inner,
                    _ => return None,
                },
                _ => return None,
            };
        }
    }

    Some(found)
}

/// Whether a component of an index may go along `path`, which reaches the
/// fields `steps`: through embedded objects and lists of them, never a link,
/// to a scalar, a link or a list of scalars or links, with at most one list
/// on the way, and with none in an index of `components` components but
/// one.
pub(crate) fn indexable(steps: &[&FieldDef], components: usize) -> bool {
    let Some((leaf, through)) = steps.split_last() else {
        return false;
    };
    let mut lists = 0;

    for step in through {
        match &step.kind {
            Kind::Object(_) => {}
            Kind::List(element) if matches!(**element, Kind::Object(_)) => lists += 1,
            _ => return false,
        }
    }

    let leaf_ok = match &leaf.kind {
        kind if kind.is_scalar() => true,
        Kind::List(element) if element.is_scalar() => {
            lists += 1;
            true
        }
        _ => false,
    };

    leaf_ok && lists <= 1 && (lists == 0 || components == 1)
}

/// A collection, as the file knows it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CollectionDef {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) fields: Fields,
    /// The id of the primary key's field.
    pub(crate) key: u64,
    /// Whether the primary key is assigned by the engine.
    pub(crate) auto: bool,
    pub(crate) indexes: Vec<IndexDef>,
}

impl CollectionDef {
    /// The primary key's field.
    pub(crate) fn key_field(&self) -> Option<&FieldDef> {
        self.fields.by_id(self.key)
    }
}

/// A cascading link into a collection: what deleting one of the collection's
/// objects has to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cascade {
    /// The position of the collection whose objects hold the link.
    pub(crate) from: usize,
    /// The link field's id.
    pub(crate) field: u64,
    /// Where among that collection's indexes the one that begins with the
    /// link field is, if it has one.
    pub(crate) index: Option<usize>,
}

/// The stored schema a database handle opened the file with, and its record
/// as the file held it. An object transaction compares that record with the
/// file's, to notice another process's migration.
#[derive(Debug)]
pub(crate) struct OpenSchema {
    pub(crate) schema: StoredSchema,
    pub(crate) encoded: Vec<u8>,
    /// The order of each collection's fields by name, at the collection's
    /// position, so that an object read alone has its fields put in their
    /// places rather than sorted. Worked out once, here: the schema a handle
    /// opened a file with does not change while the handle lives, where the
    /// fields of a schema being migrated are renamed in place.
    orders: Vec<NameOrder>,
    /// The names of each collection's trees, at its position, made once
    /// here for the same reason.
    names: Vec<CollectionNames>,
    /// The cascading links into each collection, at its position, so that a
    /// delete in a collection nothing cascades into asks one question of a
    /// slice rather than walking the schema.
    cascades: Vec<Vec<Cascade>>,
    /// How each Rust type a transaction of the handle reached its collection
    /// as lies in it, by the type's id: worked out from the type and the
    /// stored schema the first time, and kept for the life of the schema.
    /// The object layer above knows what each holds.
    typed: Mutex<Vec<(TypeId, Arc<dyn Any + Send + Sync>)>>,
    /// How each type a language binding declares at run time lies in a
    /// collection, by the key the binding gives the type and the
    /// collection's position, kept as `typed` is.
    shapes: Mutex<Vec<(ShapeKey, Kept)>>,
}

/// What a binding's type's shape is kept under: the binding's key for the
/// type, and the collection's position.
type ShapeKey = (u64, usize);

/// Something the object layer above keeps with the schema.
type Kept = Arc<dyn Any + Send + Sync>;

impl OpenSchema {
    pub(crate) fn new(schema: StoredSchema, encoded: Vec<u8>) -> Self {
        let orders = schema
            .collections
            .iter()
            .map(|collection| NameOrder::of(&collection.fields))
            .collect();
        let names = schema.collections.iter().map(CollectionNames::of).collect();
        let cascades = cascades_of(&schema);

        Self {
            schema,
            encoded,
            orders,
            names,
            cascades,
            typed: Mutex::new(Vec::new()),
            shapes: Mutex::new(Vec::new()),
        }
    }

    /// The order of the fields by name of the collection at `position`.
    pub(crate) fn order(&self, position: usize) -> Option<&NameOrder> {
        self.orders.get(position)
    }

    /// The names of the trees of the collection at `position`, which is the
    /// position of one of the schema's collections.
    pub(crate) fn names(&self, position: usize) -> &CollectionNames {
        &self.names[position]
    }

    /// The cascading links into the collection at `position`, which is the
    /// position of one of the schema's collections.
    pub(crate) fn cascades(&self, position: usize) -> &[Cascade] {
        &self.cascades[position]
    }

    /// What [`keep_typed`](Self::keep_typed) kept for the type `id`.
    pub(crate) fn typed(&self, id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.typed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(kept, _)| *kept == id)
            .map(|(_, typed)| Arc::clone(typed))
    }

    /// What [`keep_shape`](Self::keep_shape) kept for `key`.
    pub(crate) fn shape(&self, key: ShapeKey) -> Option<Kept> {
        self.shapes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(kept, _)| *kept == key)
            .map(|(_, shape)| Arc::clone(shape))
    }

    /// Keeps `shape` for `key`, unless another thread kept one first.
    pub(crate) fn keep_shape(&self, key: ShapeKey, shape: Kept) {
        let mut kept = self.shapes.lock().unwrap_or_else(PoisonError::into_inner);

        if kept.iter().all(|(other, _)| *other != key) {
            kept.push((key, shape));
        }
    }

    /// Keeps `typed` for the type `id`, unless another thread kept one
    /// first, which is the same.
    pub(crate) fn keep_typed(&self, id: TypeId, typed: Arc<dyn Any + Send + Sync>) {
        let mut kept = self.typed.lock().unwrap_or_else(PoisonError::into_inner);

        if kept.iter().all(|(other, _)| *other != id) {
            kept.push((id, typed));
        }
    }
}

/// The cascading links of `schema`, by the position of the collection each
/// links to. An index on the link field alone is the shortest to walk; one
/// that begins with the field finds the same objects.
fn cascades_of(schema: &StoredSchema) -> Vec<Vec<Cascade>> {
    let mut cascades = vec![Vec::new(); schema.collections.len()];

    for (from, collection) in schema.collections.iter().enumerate() {
        for field in collection.fields.list.iter().filter(|field| field.cascade) {
            let Kind::Link { collection: target } = field.kind else {
                continue;
            };
            let Some(target) = schema
                .collections
                .iter()
                .position(|collection| collection.id == target)
            else {
                continue;
            };
            let index = collection
                .indexes
                .iter()
                .position(|index| index.is_on(field.id))
                .or_else(|| {
                    collection.indexes.iter().position(|index| {
                        index
                            .components
                            .first()
                            .is_some_and(|path| *path == [field.id])
                    })
                });

            cascades[target].push(Cascade {
                from,
                field: field.id,
                index,
            });
        }
    }

    cascades
}

/// The schema a file holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredSchema {
    /// The object format the schema is stored in, which never goes down:
    /// records may hold what a schema of a later version no longer declares.
    pub(crate) format: i64,
    pub(crate) version: u64,
    pub(crate) collections: Vec<CollectionDef>,
    pub(crate) next_collection: u64,
    pub(crate) next_index: u64,
}

impl StoredSchema {
    pub(crate) fn collection(&self, name: &str) -> Option<&CollectionDef> {
        self.collections
            .iter()
            .find(|collection| collection.name == name)
    }

    pub(crate) fn collection_by_id(&self, id: u64) -> Option<&CollectionDef> {
        self.collections
            .iter()
            .find(|collection| collection.id == id)
    }

    /// The kind of the primary key of the collection with id `id`, which is
    /// what a link to it holds.
    pub(crate) fn key_kind(&self, id: u64) -> Option<Kind> {
        self.collection_by_id(id)
            .and_then(CollectionDef::key_field)
            .map(|field| field.kind.clone())
    }

    /// The lowest object format that holds what the schema declares: 2 for
    /// a date or a list of embedded objects anywhere in it, an index that is
    /// not on one field of its collection, or a cascading link, and 1
    /// otherwise.
    pub(crate) fn needs(&self) -> i64 {
        fn needs_kind(kind: &Kind) -> i64 {
            match kind {
                Kind::Date => 2,
                Kind::List(element) if matches!(**element, Kind::Object(_)) => 2,
                Kind::List(element) => needs_kind(element),
                Kind::Object(fields) => needs_fields(fields),
                _ => OLDEST_OBJECT_FORMAT,
            }
        }

        fn needs_fields(fields: &Fields) -> i64 {
            fields
                .list
                .iter()
                .map(|field| needs_kind(&field.kind))
                .max()
                .unwrap_or(OLDEST_OBJECT_FORMAT)
        }

        self.collections
            .iter()
            .map(|collection| {
                let plain = collection
                    .indexes
                    .iter()
                    .all(|index| index.field().is_some())
                    && collection.fields.list.iter().all(|field| !field.cascade);
                let declared = if plain { OLDEST_OBJECT_FORMAT } else { 2 };

                needs_fields(&collection.fields).max(declared)
            })
            .max()
            .unwrap_or(OLDEST_OBJECT_FORMAT)
    }

    /// The record of the stored schema.
    pub(crate) fn encode(&self) -> Vec<u8> {
        codec::write(&[
            (1, Raw::Int(self.format)),
            (2, int(self.version)),
            (
                3,
                Raw::List(self.collections.iter().map(encode_collection).collect()),
            ),
            (4, int(self.next_collection)),
            (5, int(self.next_index)),
        ])
    }

    /// Reads a stored schema. `Err(None)` is a record in an object format this
    /// build does not know, and `Err(Some(reason))` a damaged one.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, Option<&'static str>> {
        let raw = codec::read(bytes).map_err(Some)?;
        let fields = Record(&raw);
        let format = fields.int(1).map_err(Some)?;

        if !(OLDEST_OBJECT_FORMAT..=OBJECT_FORMAT).contains(&format) {
            return Err(None);
        }

        let schema = Self {
            format,
            version: fields.count(2).map_err(Some)?,
            collections: fields
                .list(3)
                .map_err(Some)?
                .iter()
                .map(decode_collection)
                .collect::<Result<_, _>>()
                .map_err(Some)?,
            next_collection: fields.count(4).map_err(Some)?,
            next_index: fields.count(5).map_err(Some)?,
        };

        schema.check().map_err(Some)?;

        if schema.needs() > format {
            return Err(Some(
                "the schema declares what its object format does not have",
            ));
        }

        Ok(schema)
    }

    /// The object format a stored schema's record names, when it names one:
    /// what [`decode`](Self::decode) refuses as unknown.
    pub(crate) fn format_of(bytes: &[u8]) -> Option<i64> {
        codec::read(bytes)
            .ok()
            .and_then(|raw| Record(&raw).int(1).ok())
    }

    /// The rules a stored schema keeps: ids below the next ones and unique,
    /// keys and links that point at something, and indexes on fields that
    /// exist.
    fn check(&self) -> Result<(), &'static str> {
        let mut collection_ids = Vec::new();
        let mut index_ids = Vec::new();

        for collection in &self.collections {
            if collection.id >= self.next_collection || collection_ids.contains(&collection.id) {
                return Err("the schema's collection ids are inconsistent");
            }

            collection_ids.push(collection.id);
            check_fields(&collection.fields, self)?;

            if collection
                .fields
                .list
                .iter()
                .any(|field| field.cascade && !matches!(field.kind, Kind::Link { .. }))
            {
                return Err("a field that is not a link cascades");
            }

            if !collection
                .key_field()
                .is_some_and(|field| field.kind.is_key() && !field.optional)
            {
                return Err("a collection's primary key is not a key field");
            }

            for index in &collection.indexes {
                if index.id >= self.next_index || index_ids.contains(&index.id) {
                    return Err("the schema's index ids are inconsistent");
                }

                index_ids.push(index.id);

                let count = index.components.len();

                if count == 0 || count > MAX_COMPONENTS {
                    return Err("an index has no components, or too many");
                }

                for component in &index.components {
                    let fits = path_fields(&collection.fields, component)
                        .is_some_and(|steps| indexable(&steps, count));

                    if !fits {
                        return Err("an index is on a field that cannot have one");
                    }
                }
            }
        }

        Ok(())
    }
}

fn check_fields(fields: &Fields, schema: &StoredSchema) -> Result<(), &'static str> {
    let mut ids = Vec::new();

    for field in &fields.list {
        if field.id >= fields.next_id || ids.last().is_some_and(|last| *last >= field.id) {
            return Err("the schema's field ids are inconsistent");
        }

        ids.push(field.id);
        check_kind(&field.kind, schema)?;

        if field
            .default
            .as_ref()
            .is_some_and(|default| !default_fits(default, &field.kind))
        {
            return Err("a field's default does not have the field's type");
        }
    }

    Ok(())
}

/// Whether `value` can be the default of a field of `kind`: a scalar of that
/// type, or a list of them.
fn default_fits(value: &Value, kind: &Kind) -> bool {
    match (kind, value) {
        (Kind::Bool, Value::Bool(_))
        | (Kind::Int, Value::Int(_))
        | (Kind::Float, Value::Float(_))
        | (Kind::String, Value::String(_))
        | (Kind::Bytes, Value::Bytes(_))
        | (Kind::Date, Value::Date(_)) => true,
        (Kind::List(element), Value::List(values)) => {
            values.iter().all(|value| default_fits(value, element))
        }
        _ => false,
    }
}

fn check_kind(kind: &Kind, schema: &StoredSchema) -> Result<(), &'static str> {
    match kind {
        Kind::Link { collection } if schema.collection_by_id(*collection).is_none() => {
            Err("a link points at no collection")
        }
        Kind::List(element) if matches!(**element, Kind::List(_)) => Err("a list holds lists"),
        Kind::List(element) => check_kind(element, schema),
        Kind::Object(fields) if fields.list.iter().any(|field| field.cascade) => {
            Err("a field inside an embedded object cascades")
        }
        Kind::Object(fields) => check_fields(fields, schema),
        _ => Ok(()),
    }
}

fn int(value: u64) -> Raw {
    Raw::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn encode_collection(collection: &CollectionDef) -> Raw {
    Raw::Object(vec![
        (1, int(collection.id)),
        (2, Raw::String(collection.name.clone())),
        (3, encode_fields(&collection.fields.list)),
        (4, int(collection.fields.next_id)),
        (5, int(collection.key)),
        (6, Raw::Bool(collection.auto)),
        (
            7,
            Raw::List(
                collection
                    .indexes
                    .iter()
                    .map(|index| {
                        // An index on one field of the collection keeps format
                        // 1's field 2; any other lists its components' paths.
                        Raw::Object(match index.field() {
                            Some(field) => vec![
                                (1, int(index.id)),
                                (2, int(field)),
                                (3, Raw::Bool(index.unique)),
                            ],
                            None => vec![
                                (1, int(index.id)),
                                (3, Raw::Bool(index.unique)),
                                (
                                    4,
                                    Raw::List(
                                        index
                                            .components
                                            .iter()
                                            .map(|path| {
                                                Raw::Object(vec![(
                                                    1,
                                                    Raw::List(
                                                        path.iter().map(|id| int(*id)).collect(),
                                                    ),
                                                )])
                                            })
                                            .collect(),
                                    ),
                                ),
                            ],
                        })
                    })
                    .collect(),
            ),
        ),
    ])
}

fn encode_fields(fields: &[FieldDef]) -> Raw {
    Raw::List(
        fields
            .iter()
            .map(|field| {
                let mut raw = vec![
                    (1, int(field.id)),
                    (2, Raw::String(field.name.clone())),
                    (3, encode_kind(&field.kind)),
                    (4, Raw::Bool(field.optional)),
                ];

                if let Some(default) = &field.default {
                    if let Some(value) = default_raw(default) {
                        raw.push((5, value));
                    }
                }

                // Only when set, so that a schema of format 1 keeps format
                // 1's bytes.
                if field.cascade {
                    raw.push((6, Raw::Bool(true)));
                }

                Raw::Object(raw)
            })
            .collect(),
    )
}

/// A default as the record holds it: a scalar, or a list of scalars.
fn default_raw(value: &Value) -> Option<Raw> {
    Some(match value {
        Value::Bool(value) => Raw::Bool(*value),
        Value::Int(value) => Raw::Int(*value),
        Value::Float(value) => Raw::Float(*value),
        Value::String(value) => Raw::String(value.clone()),
        Value::Bytes(value) => Raw::Bytes(value.clone()),
        Value::Date(value) => Raw::Date(*value),
        Value::List(values) => Raw::List(values.iter().map(default_raw).collect::<Option<_>>()?),
        Value::Null | Value::Object(_) => return None,
    })
}

fn default_value(raw: &Raw) -> Result<Value, &'static str> {
    Ok(match raw {
        Raw::Bool(value) => Value::Bool(*value),
        Raw::Int(value) => Value::Int(*value),
        Raw::Float(value) => Value::Float(*value),
        Raw::String(value) => Value::String(value.clone()),
        Raw::Bytes(value) => Value::Bytes(value.clone()),
        Raw::Date(value) => Value::Date(*value),
        Raw::List(values) => {
            Value::List(values.iter().map(default_value).collect::<Result<_, _>>()?)
        }
        Raw::Object(_) | Raw::Link(_) => return Err("a default is not a plain value"),
    })
}

fn encode_kind(kind: &Kind) -> Raw {
    let mut raw = vec![(1, Raw::Int(kind.code()))];

    match kind {
        Kind::Link { collection } => raw.push((2, int(*collection))),
        Kind::List(element) => raw.push((3, encode_kind(element))),
        Kind::Object(fields) => {
            raw.push((4, encode_fields(&fields.list)));
            raw.push((5, int(fields.next_id)));
        }
        _ => {}
    }

    Raw::Object(raw)
}

/// The fields of a record in the stored schema, looked up by id.
struct Record<'a>(&'a [(u64, Raw)]);

impl Record<'_> {
    fn get(&self, id: u64) -> Result<&Raw, &'static str> {
        self.0
            .iter()
            .find(|(field, _)| *field == id)
            .map(|(_, value)| value)
            .ok_or("the schema lacks a field")
    }

    fn int(&self, id: u64) -> Result<i64, &'static str> {
        match self.get(id)? {
            Raw::Int(value) => Ok(*value),
            _ => Err("the schema holds a field of the wrong type"),
        }
    }

    fn count(&self, id: u64) -> Result<u64, &'static str> {
        u64::try_from(self.int(id)?).map_err(|_| "the schema holds a negative number")
    }

    fn bool(&self, id: u64) -> Result<bool, &'static str> {
        match self.get(id)? {
            Raw::Bool(value) => Ok(*value),
            _ => Err("the schema holds a field of the wrong type"),
        }
    }

    fn string(&self, id: u64) -> Result<String, &'static str> {
        match self.get(id)? {
            Raw::String(value) => Ok(value.clone()),
            _ => Err("the schema holds a field of the wrong type"),
        }
    }

    fn list(&self, id: u64) -> Result<&[Raw], &'static str> {
        match self.get(id)? {
            Raw::List(values) => Ok(values),
            _ => Err("the schema holds a field of the wrong type"),
        }
    }

    fn object(&self, id: u64) -> Result<Record<'_>, &'static str> {
        match self.get(id)? {
            Raw::Object(fields) => Ok(Record(fields)),
            _ => Err("the schema holds a field of the wrong type"),
        }
    }
}

fn as_record(raw: &Raw) -> Result<Record<'_>, &'static str> {
    match raw {
        Raw::Object(fields) => Ok(Record(fields)),
        _ => Err("the schema holds a field of the wrong type"),
    }
}

fn decode_collection(raw: &Raw) -> Result<CollectionDef, &'static str> {
    let record = as_record(raw)?;

    Ok(CollectionDef {
        id: record.count(1)?,
        name: record.string(2)?,
        fields: Fields {
            list: decode_fields(record.list(3)?)?,
            next_id: record.count(4)?,
        },
        key: record.count(5)?,
        auto: record.bool(6)?,
        indexes: record
            .list(7)?
            .iter()
            .map(|raw| {
                let index = as_record(raw)?;

                let components = match index.get(4) {
                    // Field 2 and field 4 are two forms of one thing, so a
                    // record with both is damaged.
                    Ok(_) if index.get(2).is_ok() => {
                        return Err("an index has both a field and components");
                    }
                    Ok(Raw::List(components)) => components
                        .iter()
                        .map(|component| {
                            as_record(component)?
                                .list(1)?
                                .iter()
                                .map(|id| match id {
                                    Raw::Int(id) => u64::try_from(*id)
                                        .map_err(|_| "the schema holds a negative number"),
                                    _ => Err("the schema holds a field of the wrong type"),
                                })
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Ok(_) => return Err("the schema holds a field of the wrong type"),
                    Err(_) => vec![vec![index.count(2)?]],
                };

                Ok(IndexDef {
                    id: index.count(1)?,
                    components,
                    unique: index.bool(3)?,
                })
            })
            .collect::<Result<_, &'static str>>()?,
    })
}

fn decode_fields(raw: &[Raw]) -> Result<Vec<FieldDef>, &'static str> {
    raw.iter()
        .map(|raw| {
            let field = as_record(raw)?;

            Ok(FieldDef {
                id: field.count(1)?,
                name: field.string(2)?,
                kind: decode_kind(&field.object(3)?)?,
                optional: field.bool(4)?,
                default: field.get(5).ok().map(default_value).transpose()?,
                cascade: match field.get(6) {
                    Ok(_) => field.bool(6)?,
                    Err(_) => false,
                },
            })
        })
        .collect()
}

fn decode_kind(record: &Record<'_>) -> Result<Kind, &'static str> {
    Ok(match record.int(1)? {
        1 => Kind::Bool,
        2 => Kind::Int,
        3 => Kind::Float,
        4 => Kind::String,
        5 => Kind::Bytes,
        6 => Kind::Link {
            collection: record.count(2)?,
        },
        7 => Kind::List(Box::new(decode_kind(&record.object(3)?)?)),
        8 => Kind::Object(Fields {
            list: decode_fields(record.list(4)?)?,
            next_id: record.count(5)?,
        }),
        9 => Kind::Date,
        _ => return Err("the schema names an unknown kind"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> StoredSchema {
        let address = Fields {
            list: vec![FieldDef {
                id: 1,
                name: "city".into(),
                kind: Kind::String,
                optional: false,
                default: Some(Value::String("Seoul".into())),
                cascade: false,
            }],
            next_id: 2,
        };

        StoredSchema {
            format: OLDEST_OBJECT_FORMAT,
            version: 3,
            collections: vec![
                CollectionDef {
                    id: 1,
                    name: "users".into(),
                    fields: Fields {
                        list: vec![
                            FieldDef {
                                id: 1,
                                name: "id".into(),
                                kind: Kind::Int,
                                optional: false,
                                default: None,
                                cascade: false,
                            },
                            FieldDef {
                                id: 3,
                                name: "tags".into(),
                                kind: Kind::List(Box::new(Kind::String)),
                                optional: true,
                                default: Some(Value::List(vec![Value::from("new")])),
                                cascade: false,
                            },
                            FieldDef {
                                id: 4,
                                name: "address".into(),
                                kind: Kind::Object(address),
                                optional: true,
                                default: None,
                                cascade: false,
                            },
                        ],
                        next_id: 5,
                    },
                    key: 1,
                    auto: true,
                    indexes: vec![IndexDef {
                        id: 2,
                        components: vec![vec![3]],
                        unique: false,
                    }],
                },
                CollectionDef {
                    id: 4,
                    name: "posts".into(),
                    fields: Fields {
                        list: vec![
                            FieldDef {
                                id: 1,
                                name: "slug".into(),
                                kind: Kind::String,
                                optional: false,
                                default: None,
                                cascade: false,
                            },
                            FieldDef {
                                id: 2,
                                name: "author".into(),
                                kind: Kind::Link { collection: 1 },
                                optional: false,
                                default: None,
                                cascade: false,
                            },
                        ],
                        next_id: 3,
                    },
                    key: 1,
                    auto: false,
                    indexes: vec![IndexDef {
                        id: 5,
                        components: vec![vec![2]],
                        unique: true,
                    }],
                },
            ],
            next_collection: 5,
            next_index: 6,
        }
    }

    #[test]
    fn a_schema_reads_back_as_it_was_written() {
        let schema = sample();

        assert_eq!(StoredSchema::decode(&schema.encode()), Ok(schema));
    }

    #[test]
    fn an_inconsistent_schema_is_damaged() {
        let mut broken = Vec::new();

        let mut schema = sample();
        schema.next_collection = 4;
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].fields.list[1].default = Some(Value::List(vec![Value::Int(1)]));
        broken.push(schema);

        let mut schema = sample();
        schema.collections[1].fields.list[1].kind = Kind::Link { collection: 9 };
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].indexes[0].components = vec![vec![4]];
        broken.push(schema);

        // Through a list of strings, past a scalar, and none at all.
        let mut schema = sample();
        schema.collections[0].indexes[0].components = vec![vec![3, 1]];
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].indexes[0].components = vec![vec![1, 1]];
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].indexes[0].components = Vec::new();
        broken.push(schema);

        // A list in an index of two components.
        let mut schema = sample();
        schema.collections[0].indexes[0].components = vec![vec![1], vec![3]];
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].key = 3;
        broken.push(schema);

        // A cascade anywhere but on a link of the collection itself.
        let mut schema = sample();
        schema.format = 2;
        schema.collections[1].fields.list[0].cascade = true;
        broken.push(schema);

        let mut schema = sample();
        schema.format = 2;
        schema.collections[0].fields.list[1].kind =
            Kind::List(Box::new(Kind::Link { collection: 1 }));
        schema.collections[0].fields.list[1].default = None;
        schema.collections[0].fields.list[1].cascade = true;
        broken.push(schema);

        let mut schema = sample();
        schema.format = 2;
        let Kind::Object(address) = &mut schema.collections[0].fields.list[2].kind else {
            unreachable!()
        };
        address.list[0].kind = Kind::Link { collection: 1 };
        address.list[0].default = None;
        address.list[0].cascade = true;
        broken.push(schema);

        for schema in broken {
            assert!(
                matches!(StoredSchema::decode(&schema.encode()), Err(Some(_))),
                "{schema:?}"
            );
        }
    }

    #[test]
    fn another_object_format_is_told_apart_from_damage() {
        for format in [OLDEST_OBJECT_FORMAT - 1, OBJECT_FORMAT + 1] {
            let mut raw = codec::read(&sample().encode()).unwrap();

            raw[0].1 = Raw::Int(format);

            assert_eq!(StoredSchema::decode(&codec::write(&raw)), Err(None));
        }
    }

    #[test]
    fn a_schema_is_in_the_lowest_format_that_holds_it_or_a_higher_one() {
        let mut schema = sample();

        assert_eq!(schema.needs(), 1);
        assert_eq!(StoredSchema::decode(&schema.encode()), Ok(schema.clone()));

        // A schema of format 1 keeps format 1's bytes, which builds before
        // format 2 read.
        assert_eq!(codec::read(&schema.encode()).unwrap()[0], (1, Raw::Int(1)));

        // A format kept higher than the schema needs reads back as it is.
        schema.format = 2;
        assert_eq!(StoredSchema::decode(&schema.encode()), Ok(schema.clone()));

        // A date, inside a list inside an embedded object, needs format 2,
        // and a record of format 1 that declares one is damaged.
        let Kind::Object(address) = &mut schema.collections[0].fields.list[2].kind else {
            unreachable!()
        };

        address.list.push(FieldDef {
            id: 2,
            name: "seen".into(),
            kind: Kind::List(Box::new(Kind::Date)),
            optional: false,
            default: Some(Value::List(vec![Value::Date(-1)])),
            cascade: false,
        });
        address.next_id = 3;

        assert_eq!(schema.needs(), 2);
        assert_eq!(StoredSchema::decode(&schema.encode()), Ok(schema.clone()));

        schema.format = 1;
        assert!(matches!(
            StoredSchema::decode(&schema.encode()),
            Err(Some(_))
        ));
    }

    #[test]
    fn a_cascading_link_needs_format_2_and_reads_back() {
        let mut schema = sample();

        schema.collections[1].fields.list[1].cascade = true;

        assert_eq!(schema.needs(), 2);
        assert!(matches!(
            StoredSchema::decode(&schema.encode()),
            Err(Some(_))
        ));

        schema.format = 2;

        let decoded = StoredSchema::decode(&schema.encode()).unwrap();

        assert_eq!(decoded, schema);

        let open = OpenSchema::new(decoded, Vec::new());

        assert_eq!(
            open.cascades(0),
            [Cascade {
                from: 1,
                field: 2,
                index: Some(0),
            }]
        );
        assert!(open.cascades(1).is_empty());
    }

    #[test]
    fn a_cascade_reads_an_index_that_begins_with_its_link() {
        let mut schema = sample();

        schema.format = 2;
        schema.collections[1].fields.list[1].cascade = true;
        schema.collections[1].indexes = vec![
            IndexDef {
                id: 5,
                components: vec![vec![1], vec![2]],
                unique: false,
            },
            IndexDef {
                id: 6,
                components: vec![vec![2], vec![1]],
                unique: false,
            },
        ];
        schema.next_index = 7;

        assert_eq!(
            OpenSchema::new(schema.clone(), Vec::new()).cascades(0)[0].index,
            Some(1)
        );

        schema.collections[1].indexes.push(IndexDef {
            id: 7,
            components: vec![vec![2]],
            unique: false,
        });
        schema.next_index = 8;

        assert_eq!(
            OpenSchema::new(schema, Vec::new()).cascades(0)[0].index,
            Some(2),
            "the index on the link alone is the shortest"
        );
    }
}
