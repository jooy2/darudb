//! The stored schema: collections, fields and indexes with the ids the file
//! gives them, and its encoding as a record (`design/objects.md`, "The stored
//! schema").

use super::codec::{self, NameOrder, Raw};
use super::value::Value;

/// The object layer's format, field 1 of the stored schema.
pub(crate) const OBJECT_FORMAT: i64 = 1;

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
    /// Values of the element kind, a scalar or a link.
    List(Box<Kind>),
    /// An embedded object with these fields.
    Object(Fields),
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
            Kind::Bool | Kind::Int | Kind::Float | Kind::String | Kind::Bytes | Kind::Link { .. }
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

/// An index on one field of a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexDef {
    pub(crate) id: u64,
    pub(crate) field: u64,
    pub(crate) unique: bool,
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
}

impl OpenSchema {
    pub(crate) fn new(schema: StoredSchema, encoded: Vec<u8>) -> Self {
        let orders = schema
            .collections
            .iter()
            .map(|collection| NameOrder::of(&collection.fields))
            .collect();

        Self {
            schema,
            encoded,
            orders,
        }
    }

    /// The order of the fields by name of the collection at `position`.
    pub(crate) fn order(&self, position: usize) -> Option<&NameOrder> {
        self.orders.get(position)
    }
}

/// The schema a file holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredSchema {
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

    /// The record of the stored schema.
    pub(crate) fn encode(&self) -> Vec<u8> {
        codec::write(&[
            (1, Raw::Int(OBJECT_FORMAT)),
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

        if fields.int(1).map_err(Some)? != OBJECT_FORMAT {
            return Err(None);
        }

        let schema = Self {
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

                let indexable = collection.fields.by_id(index.field).is_some_and(|field| {
                    field.kind.is_scalar()
                        || matches!(&field.kind, Kind::List(element) if element.is_scalar())
                });

                if !indexable {
                    return Err("an index is on a field that cannot have one");
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
        | (Kind::Bytes, Value::Bytes(_)) => true,
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
        Kind::List(element) if !element.is_scalar() => Err("a list holds lists or objects"),
        Kind::List(element) => check_kind(element, schema),
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
                        Raw::Object(vec![
                            (1, int(index.id)),
                            (2, int(index.field)),
                            (3, Raw::Bool(index.unique)),
                        ])
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

                Ok(IndexDef {
                    id: index.count(1)?,
                    field: index.count(2)?,
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
            }],
            next_id: 2,
        };

        StoredSchema {
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
                            },
                            FieldDef {
                                id: 3,
                                name: "tags".into(),
                                kind: Kind::List(Box::new(Kind::String)),
                                optional: true,
                                default: Some(Value::List(vec![Value::from("new")])),
                            },
                            FieldDef {
                                id: 4,
                                name: "address".into(),
                                kind: Kind::Object(address),
                                optional: true,
                                default: None,
                            },
                        ],
                        next_id: 5,
                    },
                    key: 1,
                    auto: true,
                    indexes: vec![IndexDef {
                        id: 2,
                        field: 3,
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
                            },
                            FieldDef {
                                id: 2,
                                name: "author".into(),
                                kind: Kind::Link { collection: 1 },
                                optional: false,
                                default: None,
                            },
                        ],
                        next_id: 3,
                    },
                    key: 1,
                    auto: false,
                    indexes: vec![IndexDef {
                        id: 5,
                        field: 2,
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
        schema.collections[0].indexes[0].field = 4;
        broken.push(schema);

        let mut schema = sample();
        schema.collections[0].key = 3;
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
        let mut raw = codec::read(&sample().encode()).unwrap();

        raw[0].1 = Raw::Int(OBJECT_FORMAT + 1);

        assert_eq!(StoredSchema::decode(&codec::write(&raw)), Err(None));
    }
}
