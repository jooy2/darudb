//! The derive macros of DaruDB, which the `darudb` crate re-exports with its
//! `derive` feature: `#[derive(Object)]` implements `darudb::CollectionType`
//! for a struct, and `#[derive(Embedded)]` implements `darudb::EmbeddedType`
//! and `darudb::FieldType`. Use them from `darudb`, which documents them.
//!
//! The code they generate names the `darudb` crate by its path, `::darudb`,
//! and calls only its public API and `darudb::__derive`, so a struct's
//! fields are declared, written and read by the same functions a
//! hand-written implementation would call.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitStr, Type, parse_macro_input};

/// Makes a struct the objects of a collection: implements
/// `darudb::CollectionType`, which declares the collection, writes the
/// struct's values as records and reads records straight into it.
///
/// ```no_run
/// use darudb::{Collection, Link, Object, OpenOptions, Schema};
///
/// #[derive(Object, Debug, PartialEq)]
/// #[darudb(collection = "people")]
/// struct Person {
///     // The auto-increment key: `None` until the object is inserted.
///     id: Option<i64>,
///     name: String,
///     #[darudb(unique)]
///     email: Option<String>,
///     #[darudb(index, default = 0)]
///     age: i64,
///     tags: Vec<String>,
///     friend: Option<Link<Person>>,
/// }
///
/// let db = OpenOptions::new()
///     .schema(Schema::new(1).collection(Collection::of::<Person>()))
///     .open("app.darudb")?;
/// let mut txn = db.begin_write()?;
/// let id = txn.collection_of::<Person>()?.insert(&Person {
///     id: None,
///     name: "Ada".to_owned(),
///     email: None,
///     age: 36,
///     tags: vec!["math".to_owned()],
///     friend: None,
/// })?;
/// txn.commit()?;
///
/// let read = db.begin_read()?;
/// let ada = read.collection_of::<Person>()?.get(id)?;
///
/// assert_eq!(ada.map(|person| person.name), Some("Ada".to_owned()));
///
/// // One field, named through the constant the derive declares for it.
/// let mut txn = db.begin_write()?;
/// let mut people = txn.collection_of::<Person>()?;
///
/// people.set(id, Person::AGE, 37)?;
/// people.change(id).set(Person::NAME, "Ada Lovelace".to_owned()).set(Person::EMAIL, None).apply()?;
/// drop(people);
/// txn.commit()?;
/// # Ok::<(), darudb::Error>(())
/// ```
///
/// The struct has named fields and no generic parameters, and each field's
/// type implements `darudb::FieldType`: `bool`, `i64`, `f64`, `String`,
/// `Vec<u8>` for bytes, `Option<T>` for an optional field, `Vec<T>` for a
/// list, `darudb::Link<T>` for a link, or a struct with
/// `#[derive(Embedded)]`.
///
/// A struct attribute `#[darudb(collection = "name")]` names the
/// collection, which is named after the struct otherwise.
/// `#[darudb(index(last, first))]` on the struct declares an index on several
/// fields, named as the struct names them, and `#[darudb(unique(...))]` a
/// unique one; a string in their place is a path the collection stores, such
/// as `"address.city"`, a field inside an embedded object. Field attributes:
///
/// - `#[darudb(key)]`: the field is the primary key, of type `i64`,
///   `String` or `Vec<u8>`. Without one, the collection is keyed by an
///   auto-increment, which needs a field `id: Option<i64>`.
/// - `#[darudb(index)]`, `#[darudb(unique)]`: an index on the field, unique
///   or not.
/// - `#[darudb(rename = "name")]`: the field's name in the collection, the
///   Rust field's name otherwise.
/// - `#[darudb(default = value)]`: the value a record that leaves the field
///   out holds, anything `darudb::Value::from` takes. The field is required.
///
/// It also declares, for each field, an associated constant of the struct
/// named after the field in capitals, a `darudb::FieldOf` with the field's
/// visibility and its name in the collection: `Person::AGE` for the field
/// `age` above. `TypedWriter::set` takes it to set that one field, and its
/// methods make the conditions of a query, which compare the field only
/// with a value of its own type:
///
/// ```
/// # use darudb::{Link, Object, Query};
/// # #[derive(Object)]
/// # #[darudb(collection = "people")]
/// # struct Person {
/// #     id: Option<i64>,
/// #     name: String,
/// #     age: i64,
/// #     friend: Option<Link<Person>>,
/// # }
/// let query = Query::new()
///     .filter(Person::AGE.ge(18).and(Person::FRIEND.then(Person::NAME).eq("Ada")))
///     .sort_by(Person::NAME);
/// # let _ = query;
/// ```
///
/// ```compile_fail
/// # use darudb::{Object, Query};
/// # #[derive(Object)]
/// # #[darudb(collection = "people")]
/// # struct Person {
/// #     id: Option<i64>,
/// #     age: i64,
/// # }
/// // An int field compared with a string does not compile.
/// let query = Query::new().filter(Person::AGE.ge("eighteen"));
/// # let _ = query;
/// ```
#[proc_macro_derive(Object, attributes(darudb))]
pub fn derive_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    object(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Makes a struct an embedded object: implements `darudb::EmbeddedType`,
/// `darudb::FieldType` and `darudb::ElementType`, so that a field of an
/// object can hold it, or a list of it.
///
/// ```
/// use darudb::{Embedded, Object};
///
/// #[derive(Embedded, Debug, PartialEq)]
/// struct Address {
///     city: String,
///     #[darudb(rename = "zip")]
///     postal_code: Option<String>,
/// }
///
/// #[derive(Object, Debug, PartialEq)]
/// struct Shop {
///     #[darudb(key)]
///     name: String,
///     address: Address,
/// }
/// ```
///
/// Its fields take the attributes of `#[derive(Object)]` except `key`,
/// `index` and `unique`: an embedded object has no key, and the collection
/// that holds it indexes its fields by their paths, as
/// `#[darudb(index("address.city"))]` on the collection's struct does. It declares a constant for each field as `#[derive(Object)]`
/// does, which a path from the field that holds the embedded object goes on
/// to: `Shop::ADDRESS.then(Address::CITY).eq("Seoul")`.
#[proc_macro_derive(Embedded, attributes(darudb))]
pub fn derive_embedded(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    embedded(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A field of the struct, with what its attributes say.
struct Field {
    ident: Ident,
    vis: syn::Visibility,
    ty: Type,
    /// The field's name in the collection.
    name: String,
    key: bool,
    index: bool,
    unique: bool,
    default: Option<Expr>,
}

/// The struct's name, its collection's name, and its fields.
struct Shape {
    ident: Ident,
    collection: String,
    fields: Vec<Field>,
    /// The indexes the struct attribute declares: each component's path in
    /// the collection, and whether the index is unique.
    indexes: Vec<(Vec<String>, bool)>,
}

/// A component of an index the struct attribute declares: a field of the
/// struct by its Rust name, or a path the collection stores.
enum Component {
    Field(Ident),
    Stored(String),
}

/// Reads the struct `input`, refusing what neither macro supports: an enum,
/// a union, a tuple struct, generic parameters and unknown attributes.
fn shape(input: &DeriveInput, what: &str) -> syn::Result<Shape> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            format!("{what} cannot have generic parameters"),
        ));
    }

    let named = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named,
            fields => {
                return Err(syn::Error::new_spanned(
                    fields,
                    format!("{what} needs named fields"),
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                format!("{what} is a struct with named fields"),
            ));
        }
    };

    let mut collection = None;
    let mut declared: Vec<(Vec<Component>, bool, proc_macro2::Span)> = Vec::new();

    for attribute in &input.attrs {
        if !attribute.path().is_ident("darudb") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("collection") {
                collection = Some(meta.value()?.parse::<LitStr>()?.value());

                Ok(())
            } else if meta.path.is_ident("index") || meta.path.is_ident("unique") {
                let unique = meta.path.is_ident("unique");
                let content;
                let mut components = Vec::new();

                syn::parenthesized!(content in meta.input);

                while !content.is_empty() {
                    if content.peek(LitStr) {
                        components.push(Component::Stored(content.parse::<LitStr>()?.value()));
                    } else {
                        components.push(Component::Field(content.call(Ident::parse_any)?));
                    }

                    if content.is_empty() {
                        break;
                    }

                    content.parse::<syn::Token![,]>()?;
                }

                declared.push((components, unique, meta.path.span()));

                Ok(())
            } else {
                Err(meta.error(
                    "unknown attribute; the struct takes `collection = \"name\"`, `index(...)` and `unique(...)`",
                ))
            }
        })?;
    }

    let mut fields = Vec::with_capacity(named.named.len());

    for field in &named.named {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(field, "a field without a name"));
        };
        let mut parsed = Field {
            name: ident.unraw().to_string(),
            ident,
            vis: field.vis.clone(),
            ty: field.ty.clone(),
            key: false,
            index: false,
            unique: false,
            default: None,
        };

        for attribute in &field.attrs {
            if !attribute.path().is_ident("darudb") {
                continue;
            }

            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("key") {
                    parsed.key = true;
                } else if meta.path.is_ident("index") {
                    parsed.index = true;
                } else if meta.path.is_ident("unique") {
                    parsed.unique = true;
                } else if meta.path.is_ident("rename") {
                    parsed.name = meta.value()?.parse::<LitStr>()?.value();
                } else if meta.path.is_ident("default") {
                    parsed.default = Some(meta.value()?.parse::<Expr>()?);
                } else {
                    return Err(meta.error(
                        "unknown attribute; a field takes `key`, `index`, `unique`, `rename = \"name\"` and `default = value`",
                    ));
                }

                Ok(())
            })?;
        }

        if fields.iter().any(|other: &Field| other.name == parsed.name) {
            return Err(syn::Error::new_spanned(
                &parsed.ident,
                format!("two fields are called `{}`", parsed.name),
            ));
        }

        fields.push(parsed);
    }

    let mut indexes = Vec::with_capacity(declared.len());

    for (components, unique, span) in declared {
        if components.is_empty() {
            return Err(syn::Error::new(span, "an index is on one field at least"));
        }

        let paths = components
            .into_iter()
            .map(|component| match component {
                Component::Stored(path) => Ok(path),
                Component::Field(ident) => fields
                    .iter()
                    .find(|field| field.ident.unraw() == ident.unraw())
                    .map(|field| field.name.clone())
                    .ok_or_else(|| {
                        syn::Error::new_spanned(&ident, "the struct has no field of this name")
                    }),
            })
            .collect::<syn::Result<Vec<_>>>()?;

        indexes.push((paths, unique));
    }

    Ok(Shape {
        collection: collection.unwrap_or_else(|| input.ident.unraw().to_string()),
        ident: input.ident.clone(),
        fields,
        indexes,
    })
}

/// The methods both traits share: `write_field`, which writes the field of
/// a slot, and `read`, which reads every field into a variable of its own
/// and builds the struct from them. `slots` are the fields in slot order.
fn methods(slots: &[&Field]) -> Tokens {
    let numbers = (0..slots.len()).collect::<Vec<_>>();
    let idents = slots.iter().map(|field| &field.ident).collect::<Vec<_>>();
    let types = slots.iter().map(|field| &field.ty).collect::<Vec<_>>();
    let values = (0..slots.len())
        .map(|slot| format_ident!("field_{slot}"))
        .collect::<Vec<_>>();

    quote! {
        fn write_field(
            &self,
            slot: usize,
            value: ::darudb::ValueWriter<'_>,
        ) -> ::darudb::Result<()> {
            match slot {
                #(#numbers => ::darudb::FieldType::write(&self.#idents, value),)*
                _ => ::core::result::Result::Ok(()),
            }
        }

        fn read(mut fields: ::darudb::FieldReader<'_>) -> ::darudb::Result<Self> {
            #(let mut #values: ::core::option::Option<#types> = ::core::option::Option::None;)*

            while let ::core::option::Option::Some((slot, value)) = fields.next()? {
                match slot {
                    #(#numbers => {
                        #values = ::core::option::Option::Some(
                            <#types as ::darudb::FieldType>::read(value)?,
                        );
                    })*
                    _ => {}
                }
            }

            ::core::result::Result::Ok(Self {
                #(#idents: fields.take(#values)?,)*
            })
        }
    }
}

/// `Some(value)` as a `darudb::Value`, or `None`, for a field's default.
fn default_of(field: &Field) -> Tokens {
    match &field.default {
        Some(value) => quote! {
            ::core::option::Option::Some(::darudb::Value::from(#value))
        },
        None => quote! { ::core::option::Option::None },
    }
}

fn object(input: &DeriveInput) -> syn::Result<Tokens> {
    let shape = shape(input, "`#[derive(Object)]`")?;
    let ident = &shape.ident;
    let collection = &shape.collection;
    let mut keys = shape.fields.iter().filter(|field| field.key);
    let key = keys.next();

    if let Some(second) = keys.next() {
        return Err(syn::Error::new_spanned(
            &second.ident,
            "a collection has one primary key",
        ));
    }

    if let Some(key) = key.filter(|key| key.default.is_some()) {
        return Err(syn::Error::new_spanned(
            &key.ident,
            "the primary key cannot have a default",
        ));
    }

    // Without a declared key, the auto-increment `id` comes first, as the
    // engine lists a collection's fields, and is not declared.
    let auto = match key {
        Some(_) => None,
        None => Some(
            shape
                .fields
                .iter()
                .find(|field| field.name == "id")
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        &shape.ident,
                        "without a `#[darudb(key)]` field, the collection is keyed by an auto-increment, which needs a field `id: Option<i64>`",
                    )
                })?,
        ),
    };

    if let Some(id) = auto.filter(|id| id.index || id.unique || id.default.is_some()) {
        return Err(syn::Error::new_spanned(
            &id.ident,
            "the auto-increment `id` takes no attribute",
        ));
    }

    let declared = shape
        .fields
        .iter()
        .filter(|field| auto.is_none_or(|id| !std::ptr::eq(*field, id)))
        .collect::<Vec<_>>();
    let slots = auto
        .into_iter()
        .chain(declared.iter().copied())
        .collect::<Vec<_>>();
    let declarations = declared.iter().map(|field| {
        let ty = &field.ty;
        let name = &field.name;

        if field.key {
            quote! { let collection = ::darudb::__derive::key::<#ty>(collection, #name); }
        } else {
            let default = default_of(field);

            quote! {
                let collection = ::darudb::__derive::field::<#ty>(collection, #name, #default);
            }
        }
    });
    let indexes = declared.iter().filter_map(|field| {
        let name = &field.name;

        if field.unique {
            Some(quote! { let collection = collection.unique(#name); })
        } else if field.index {
            Some(quote! { let collection = collection.index(#name); })
        } else {
            None
        }
    });
    let composites = shape.indexes.iter().map(|(paths, unique)| {
        if *unique {
            quote! { let collection = collection.unique_on([#(#paths),*]); }
        } else {
            quote! { let collection = collection.index_on([#(#paths),*]); }
        }
    });
    let key_type = match key {
        Some(key) => {
            let ty = &key.ty;

            quote! { #ty }
        }
        None => quote! { i64 },
    };
    // The auto-increment `id` has to be an `Option<i64>`: `None` until the
    // engine assigns the number.
    let id_check = auto.map(|id| {
        let field = &id.ident;

        quote! {
            const _: () = {
                #[allow(dead_code)]
                fn id_is_an_optional_int(object: &#ident) -> &::core::option::Option<i64> {
                    &object.#field
                }
            };
        }
    });
    let methods = methods(&slots);
    let constants = constants(&slots);

    Ok(quote! {
        #id_check

        impl #ident {
            #constants
        }

        impl ::darudb::CollectionType for #ident {
            type Key = #key_type;

            const COLLECTION: &'static str = #collection;

            fn collection() -> ::darudb::Collection {
                let collection = ::darudb::Collection::new(<Self as ::darudb::CollectionType>::COLLECTION);
                #(#declarations)*
                #(#indexes)*
                #(#composites)*
                collection
            }

            #methods
        }
    })
}

/// A constant for each field, by its slot and its name in the collection,
/// for a condition, a sort or an update that names the field through the
/// type.
fn constants(slots: &[&Field]) -> Tokens {
    let constants = slots.iter().enumerate().map(|(slot, field)| {
        let vis = &field.vis;
        let ty = &field.ty;
        let name = &field.name;
        let constant = format_ident!("{}", field.ident.unraw().to_string().to_uppercase());
        let doc = format!(
            "The field `{name}`, which names it in a condition, a sort or an update."
        );

        quote! {
            #[doc = #doc]
            #vis const #constant: ::darudb::FieldOf<Self, #ty> = ::darudb::FieldOf::new(#slot, #name);
        }
    });

    quote! { #(#constants)* }
}

fn embedded(input: &DeriveInput) -> syn::Result<Tokens> {
    let shape = shape(input, "`#[derive(Embedded)]`")?;
    let ident = &shape.ident;

    if !shape.indexes.is_empty() {
        return Err(syn::Error::new_spanned(
            &shape.ident,
            "an embedded object has no index of its own; the collection holding it indexes its fields by their paths",
        ));
    }

    if let Some(field) = shape
        .fields
        .iter()
        .find(|field| field.key || field.index || field.unique)
    {
        return Err(syn::Error::new_spanned(
            &field.ident,
            "an embedded object has no key, and no index of its own; the collection holding it indexes its fields by their paths",
        ));
    }

    let slots = shape.fields.iter().collect::<Vec<_>>();
    let declarations = slots.iter().map(|field| {
        let ty = &field.ty;
        let name = &field.name;
        let default = default_of(field);

        quote! {
            let embedded = ::darudb::__derive::embedded_field::<#ty>(embedded, #name, #default);
        }
    });
    let methods = methods(&slots);
    let constants = constants(&slots);

    Ok(quote! {
        impl #ident {
            #constants
        }

        impl ::darudb::EmbeddedType for #ident {
            fn embedded() -> ::darudb::Embedded {
                let embedded = ::darudb::Embedded::new();
                #(#declarations)*
                embedded
            }

            #methods
        }

        // A list may hold embedded objects.
        impl ::darudb::ElementType for #ident {}

        impl ::darudb::FieldType for #ident {
            fn kind() -> ::darudb::Type {
                ::darudb::Type::object(<Self as ::darudb::EmbeddedType>::embedded())
            }

            fn write(&self, value: ::darudb::ValueWriter<'_>) -> ::darudb::Result<()> {
                value.object(self)
            }

            fn read(value: ::darudb::ValueReader<'_>) -> ::darudb::Result<Self> {
                value.object()
            }
        }
    })
}
