//! Comparing a declared schema with the stored one, and working out what a
//! migration changes (`design/objects.md`, "Migrations").
//!
//! Everything here is a computation over schemas. The changes to data it
//! calls for, building and dropping indexes and deleting collections, are the
//! caller's to carry out, in the same write transaction as storing the result.

use std::collections::BTreeMap;

use super::declare::{Collection, Field, Migration, MigrationFn, Schema, Type};
use crate::error::{Error, Result};
use crate::format::object::schema::{
    CollectionDef, FieldDef, Fields, IndexDef, Kind, StoredSchema,
};

/// What opening a file with a declared schema does to the stored schema.
pub(crate) enum Resolution {
    /// The stored schema is the declared one.
    Unchanged,
    /// The stored schema becomes the plan's.
    Change(Plan),
}

/// A new stored schema and the data changes that come with it.
pub(crate) struct Plan {
    /// The stored schema before the change, `None` for a file without one.
    pub(crate) from: Option<StoredSchema>,
    pub(crate) to: StoredSchema,
    /// Indexes to build from the objects already there, with their
    /// collection's id.
    pub(crate) build: Vec<(u64, IndexDef)>,
    /// Indexes whose trees go.
    pub(crate) drop: Vec<u64>,
    /// Collections whose trees go.
    pub(crate) delete: Vec<u64>,
    /// The migration functions to run, in version order.
    pub(crate) functions: Vec<MigrationFn>,
}

/// What `declared` does to `stored`, given the application's `migrations`.
pub(crate) fn resolve(
    declared: &Schema,
    stored: Option<&StoredSchema>,
    migrations: &[Migration],
) -> Result<Resolution> {
    let Some(stored) = stored else {
        let empty = StoredSchema {
            version: 0,
            collections: Vec::new(),
            next_collection: 1,
            next_index: 1,
        };
        let mut plan = plan(declared, &empty, &[])?;

        plan.from = None;
        plan.build.clear();

        return Ok(Resolution::Change(plan));
    };

    if stored.version > declared.version {
        return Err(Error::SchemaTooNew {
            stored: stored.version,
            declared: declared.version,
        });
    }

    if stored.version == declared.version {
        let plan = plan(declared, stored, &[])
            .map_err(|error| mismatch(declared.version, &error.to_string()))?;

        // Compared as records: a float default of NaN is not equal to itself
        // as a value, but it is as bytes.
        if plan.to.encode() != stored.encode() {
            return Err(mismatch(
                declared.version,
                "it differs from the schema the file holds at that version",
            ));
        }

        return Ok(Resolution::Unchanged);
    }

    let mut steps: Vec<&Migration> = migrations
        .iter()
        .filter(|migration| (stored.version + 1..=declared.version).contains(&migration.version))
        .collect();

    steps.sort_by_key(|migration| migration.version);

    if let Some(pair) = steps
        .windows(2)
        .find(|pair| pair[0].version == pair[1].version)
    {
        return Err(Error::InvalidArgument {
            message: format!("two migrations to version {}", pair[0].version),
        });
    }

    plan(declared, stored, &steps).map(Resolution::Change)
}

fn mismatch(version: u64, reason: &str) -> Error {
    Error::SchemaMismatch {
        message: format!(
            "the schema declared at version {version} does not match: {reason}; raise the version to change it"
        ),
    }
}

fn invalid(message: String) -> Error {
    Error::InvalidArgument { message }
}

/// The stored schema `declared` makes of `old`, applying `steps` first.
fn plan(declared: &Schema, old: &StoredSchema, steps: &[&Migration]) -> Result<Plan> {
    let mut renamed = old.clone();
    // Fields a step replaces, by collection id.
    let mut replaced: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut delete = Vec::new();

    for step in steps {
        apply_step(step, &mut renamed, &mut replaced, &mut delete)?;
    }

    // Collection ids first, so that links can be resolved to them.
    let mut next_collection = old.next_collection;
    let mut ids = BTreeMap::new();

    for collection in &declared.collections {
        let id = match renamed.collection(&collection.name) {
            Some(existing) => existing.id,
            None => {
                next_collection += 1;

                next_collection - 1
            }
        };

        ids.insert(collection.name.clone(), id);
    }

    if let Some(gone) = renamed
        .collections
        .iter()
        .find(|existing| !ids.contains_key(&existing.name))
    {
        return Err(invalid(format!(
            "the collection `{}` is gone from the schema; a migration has to delete it",
            gone.name
        )));
    }

    let mut next_index = old.next_index;
    let mut collections = Vec::new();
    let mut build = Vec::new();
    let mut drop = Vec::new();

    for collection in &declared.collections {
        let existing = renamed.collection(&collection.name);
        let replaced = existing
            .and_then(|existing| replaced.get(&existing.id))
            .cloned()
            .unwrap_or_default();
        let (definition, built, dropped) = merge_collection(
            collection,
            ids[&collection.name],
            existing,
            &replaced,
            &ids,
            &mut next_index,
        )?;

        build.extend(built.into_iter().map(|index| (definition.id, index)));
        drop.extend(dropped);
        collections.push(definition);
    }

    // By id, so that declaring the same collections in another order is the
    // same schema.
    collections.sort_by_key(|collection| collection.id);

    Ok(Plan {
        from: Some(old.clone()),
        to: StoredSchema {
            version: declared.version,
            collections,
            next_collection,
            next_index,
        },
        build,
        drop,
        delete,
        functions: steps.iter().filter_map(|step| step.run.clone()).collect(),
    })
}

/// Applies the renames, deletions and replacements of one migration step to
/// `schema`, naming things as they were before the step.
fn apply_step(
    step: &Migration,
    schema: &mut StoredSchema,
    replaced: &mut BTreeMap<u64, Vec<u64>>,
    delete: &mut Vec<u64>,
) -> Result<()> {
    let version = step.version;
    let find = |schema: &StoredSchema, name: &str| {
        schema
            .collections
            .iter()
            .position(|collection| collection.name == name)
            .ok_or_else(|| {
                invalid(format!(
                    "the migration to version {version} names `{name}`, which the schema before it does not have"
                ))
            })
    };

    for (collection, from, to) in &step.renamed_fields {
        let index = find(schema, collection)?;
        let fields = &mut schema.collections[index].fields.list;

        if fields.iter().any(|field| field.name == *to) {
            return Err(invalid(format!(
                "the migration to version {version} renames `{collection}.{from}` to `{to}`, which exists"
            )));
        }

        let field = fields
            .iter_mut()
            .find(|field| field.name == *from)
            .ok_or_else(|| {
                invalid(format!(
                    "the migration to version {version} renames `{collection}.{from}`, which does not exist"
                ))
            })?;

        field.name.clone_from(to);
    }

    for (collection, name) in &step.replaced_fields {
        let index = find(schema, collection)?;
        let definition = &schema.collections[index];
        let field = definition.fields.by_name(name).ok_or_else(|| {
            invalid(format!(
                "the migration to version {version} replaces `{collection}.{name}`, which does not exist"
            ))
        })?;

        if field.id == definition.key {
            return Err(invalid(format!(
                "the migration to version {version} replaces the primary key of `{collection}`, which cannot change"
            )));
        }

        replaced.entry(definition.id).or_default().push(field.id);
    }

    for name in &step.deleted_collections {
        let index = find(schema, name)?;

        delete.push(schema.collections.remove(index).id);
    }

    for (from, to) in &step.renamed_collections {
        let index = find(schema, from)?;

        if schema
            .collections
            .iter()
            .any(|collection| collection.name == *to)
        {
            return Err(invalid(format!(
                "the migration to version {version} renames `{from}` to `{to}`, which exists"
            )));
        }

        schema.collections[index].name.clone_from(to);
    }

    Ok(())
}

/// The stored definition of the declared `collection`, the indexes to build
/// and the index ids to drop.
fn merge_collection(
    collection: &Collection,
    id: u64,
    existing: Option<&CollectionDef>,
    replaced: &[u64],
    ids: &BTreeMap<String, u64>,
    next_index: &mut u64,
) -> Result<(CollectionDef, Vec<IndexDef>, Vec<u64>)> {
    let name = &collection.name;
    let fields = merge_fields(
        name,
        &collection.all_fields(),
        existing.map(|existing| &existing.fields),
        replaced,
        ids,
    )?;
    let key = fields
        .by_name(collection.key_name())
        .map(|field| field.id)
        .ok_or_else(|| invalid(format!("`{name}` has no primary key field")))?;
    let auto = collection.key.is_none();

    if let Some(existing) = existing {
        if existing.key != key || existing.auto != auto {
            return Err(invalid(format!(
                "the primary key of `{name}` changed, which a migration cannot do"
            )));
        }
    }

    let mut indexes = Vec::new();
    let mut build = Vec::new();

    for (field_name, unique) in &collection.indexes {
        let field = fields
            .by_name(field_name)
            .ok_or_else(|| {
                invalid(format!(
                    "`{name}` indexes `{field_name}`, which it does not have"
                ))
            })?
            .id;
        // A record written before its field existed reads the field's
        // default, so an index on a field whose default changed is built
        // again, with the new default for those records.
        let kept = existing.and_then(|existing| {
            let same_default = existing.fields.by_id(field).map(|old| &old.default)
                == fields.by_id(field).map(|new| &new.default);

            existing
                .indexes
                .iter()
                .find(|index| index.field == field && index.unique == *unique && same_default)
        });

        match kept {
            Some(index) => indexes.push(index.clone()),
            None => {
                let index = IndexDef {
                    id: *next_index,
                    field,
                    unique: *unique,
                };

                *next_index += 1;
                build.push(index.clone());
                indexes.push(index);
            }
        }
    }

    indexes.sort_by_key(|index| index.id);

    let drop = existing
        .map(|existing| {
            existing
                .indexes
                .iter()
                .filter(|index| !indexes.iter().any(|kept| kept.id == index.id))
                .map(|index| index.id)
                .collect()
        })
        .unwrap_or_default();

    Ok((
        CollectionDef {
            id,
            name: name.clone(),
            fields,
            key,
            auto,
            indexes,
        },
        build,
        drop,
    ))
}

/// The stored fields of `declared`, keeping the ids of `existing` fields with
/// the same name, except `replaced` ones.
fn merge_fields(
    owner: &str,
    declared: &[Field],
    existing: Option<&Fields>,
    replaced: &[u64],
    ids: &BTreeMap<String, u64>,
) -> Result<Fields> {
    let mut next_id = existing.map_or(1, |existing| existing.next_id);
    let mut list = Vec::new();

    for field in declared {
        let name = &field.name;
        let path = format!("{owner}.{name}");
        let kept = existing
            .and_then(|existing| existing.by_name(name))
            .filter(|kept| !replaced.contains(&kept.id));
        let (id, kind) = match kept {
            Some(kept) => {
                let embedded = match &kept.kind {
                    Kind::Object(fields) => Some(fields),
                    _ => None,
                };
                let kind = to_kind(&path, &field.kind, embedded, ids)?;

                if !same_shape(&kind, &kept.kind) {
                    return Err(invalid(format!(
                        "the type of `{path}` changed; a migration has to replace the field"
                    )));
                }

                if kept.optional && !field.optional && field.default.is_none() {
                    return Err(invalid(format!(
                        "`{path}` became required without a default; objects without it would have no value"
                    )));
                }

                (kept.id, kind)
            }
            None => {
                if existing.is_some() && !field.optional && field.default.is_none() {
                    return Err(invalid(format!(
                        "the new field `{path}` is required without a default; the objects already there would have no value"
                    )));
                }

                next_id += 1;

                (next_id - 1, to_kind(&path, &field.kind, None, ids)?)
            }
        };

        list.push(FieldDef {
            id,
            name: name.clone(),
            kind,
            optional: field.optional,
            default: field.default.clone(),
        });
    }

    list.sort_by_key(|field| field.id);

    Ok(Fields { list, next_id })
}

/// The stored kind of a declared type, with an embedded object's fields
/// merged into `existing`'s.
fn to_kind(
    path: &str,
    kind: &Type,
    existing: Option<&Fields>,
    ids: &BTreeMap<String, u64>,
) -> Result<Kind> {
    Ok(match kind {
        Type::Bool => Kind::Bool,
        Type::Int => Kind::Int,
        Type::Float => Kind::Float,
        Type::String => Kind::String,
        Type::Bytes => Kind::Bytes,
        Type::Link(target) => Kind::Link {
            collection: *ids.get(target).ok_or_else(|| {
                invalid(format!(
                    "`{path}` links to `{target}`, which is not a collection"
                ))
            })?,
        },
        Type::List(element) => Kind::List(Box::new(to_kind(path, element, None, ids)?)),
        Type::Object(embedded) => {
            Kind::Object(merge_fields(path, &embedded.fields, existing, &[], ids)?)
        }
    })
}

/// Whether two kinds hold the same values: equal, except that an embedded
/// object may gain and lose fields, which its merge has already checked.
fn same_shape(a: &Kind, b: &Kind) -> bool {
    match (a, b) {
        (Kind::Object(_), Kind::Object(_)) => true,
        (Kind::List(a), Kind::List(b)) => same_shape(a, b),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::object::Value;

    fn v1() -> Schema {
        Schema::new(1)
            .collection(
                Collection::new("users")
                    .field("name", Type::String)
                    .optional("email", Type::String)
                    .unique("email"),
            )
            .collection(
                Collection::new("posts")
                    .primary_key("slug", Type::String)
                    .field("author", Type::link("users"))
                    .index("author"),
            )
    }

    fn stored(schema: &Schema) -> StoredSchema {
        match resolve(schema, None, &[]).unwrap() {
            Resolution::Change(plan) => plan.to,
            Resolution::Unchanged => panic!("a first schema is stored"),
        }
    }

    fn plan_of(schema: &Schema, from: &StoredSchema, migrations: &[Migration]) -> Result<Plan> {
        match resolve(schema, Some(from), migrations)? {
            Resolution::Change(plan) => Ok(plan),
            Resolution::Unchanged => panic!("expected a change"),
        }
    }

    #[test]
    fn a_first_schema_gets_ids_in_declaration_order() {
        let schema = stored(&v1());
        let users = schema.collection("users").unwrap();
        let posts = schema.collection("posts").unwrap();

        assert_eq!((users.id, posts.id, schema.next_collection), (1, 2, 3));
        assert!(users.auto);
        assert_eq!(users.key_field().unwrap().name, "id");
        assert_eq!(
            posts.fields.by_name("author").unwrap().kind,
            Kind::Link { collection: 1 }
        );
        assert_eq!(schema.next_index, 3);
        assert_eq!(StoredSchema::decode(&schema.encode()), Ok(schema));
    }

    #[test]
    fn the_same_version_must_be_the_same_schema() {
        let schema = stored(&v1());

        assert!(matches!(
            resolve(&v1(), Some(&schema), &[]),
            Ok(Resolution::Unchanged)
        ));

        let changed =
            Schema::new(1).collection(Collection::new("users").field("name", Type::String));

        assert_eq!(
            resolve(&changed, Some(&schema), &[])
                .err()
                .map(|error| error.code()),
            Some("SCHEMA_MISMATCH")
        );

        let older = Schema::new(0);

        assert!(matches!(
            resolve(&older, Some(&schema), &[]),
            Err(Error::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn a_migration_keeps_ids_across_renames_and_adds_what_is_new() {
        let from = stored(&v1());
        let v2 = Schema::new(2)
            .collection(
                Collection::new("people")
                    .field("full_name", Type::String)
                    .optional("email", Type::String)
                    .with_default("age", Type::Int, 0)
                    .unique("email")
                    .index("age"),
            )
            .collection(
                Collection::new("posts")
                    .primary_key("slug", Type::String)
                    .field("author", Type::link("people")),
            );
        let migration = Migration::to(2)
            .rename_field("users", "name", "full_name")
            .rename_collection("users", "people");
        let plan = plan_of(&v2, &from, &[migration]).unwrap();
        let people = plan.to.collection("people").unwrap();

        assert_eq!(people.id, 1);
        assert_eq!(people.fields.by_name("full_name").unwrap().id, 2);
        assert_eq!(people.fields.by_name("age").unwrap().id, 4);
        assert_eq!(plan.build.len(), 1, "the age index is built");
        assert_eq!(plan.drop.len(), 1, "the author index goes");
        assert_eq!(
            plan.to
                .collection("posts")
                .unwrap()
                .fields
                .by_name("author")
                .unwrap()
                .kind,
            Kind::Link { collection: 1 }
        );
    }

    #[test]
    fn changes_the_engine_cannot_make_alone_are_refused() {
        let from = stored(&v1());
        // A replaced field is a new field, so a required one has a default.
        let retyped = Schema::new(2)
            .collection(
                Collection::new("users")
                    .with_default("name", Type::Int, 0)
                    .optional("email", Type::String)
                    .unique("email"),
            )
            .collection(v1().collections[1].clone());
        let dropped = Schema::new(2).collection(v1().collections[0].clone());
        let required = Schema::new(2)
            .collection(v1().collections[0].clone().field("born", Type::Int))
            .collection(v1().collections[1].clone());

        for schema in [&retyped, &dropped, &required] {
            assert_eq!(
                plan_of(schema, &from, &[]).err().map(|error| error.code()),
                Some("INVALID_ARGUMENT"),
                "{schema:?}"
            );
        }

        // Named in a migration, the same changes go through.
        let replaced = plan_of(
            &retyped,
            &from,
            &[Migration::to(2).replace_field("users", "name")],
        )
        .unwrap();

        assert_eq!(
            replaced
                .to
                .collection("users")
                .unwrap()
                .fields
                .by_name("name")
                .unwrap()
                .id,
            4
        );

        let deleted = plan_of(
            &dropped,
            &from,
            &[Migration::to(2).delete_collection("posts")],
        )
        .unwrap();

        assert_eq!(deleted.delete, [2]);
    }

    #[test]
    fn declared_schemas_are_checked() {
        let broken = [
            Schema::new(0),
            Schema::new(1)
                .collection(Collection::new("a"))
                .collection(Collection::new("a")),
            Schema::new(1).collection(Collection::new("a").field("id", Type::Int)),
            Schema::new(1).collection(Collection::new("a").primary_key("k", Type::Float)),
            Schema::new(1).collection(Collection::new("a").field("b", Type::link("nowhere"))),
            Schema::new(1).collection(
                Collection::new("a")
                    .field("b", Type::String)
                    .field("b", Type::Int),
            ),
            Schema::new(1).collection(Collection::new("a").index("missing")),
            Schema::new(1).collection(Collection::new("a").with_default("b", Type::Int, "text")),
            Schema::new(1)
                .collection(Collection::new("a").field("b", Type::list(Type::list(Type::Int)))),
        ];

        for schema in broken {
            assert_eq!(
                schema.validate().err().map(|error| error.code()),
                Some("INVALID_ARGUMENT"),
                "{schema:?}"
            );
        }

        assert!(v1().validate().is_ok());
        assert!(
            Schema::new(1)
                .collection(Collection::new("a").with_default(
                    "tags",
                    Type::list(Type::String),
                    vec![Value::from("x")]
                ))
                .validate()
                .is_ok()
        );
    }
}
