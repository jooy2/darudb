//! Conditions, sorts and aggregates that name their fields through a type:
//! the methods of [`FieldOf`], [`PathOf`] for a path through embedded
//! objects and links, and the traits that say which conditions a field's
//! type allows.
//!
//! A condition made this way is a [`Filter`] like any other, holding the
//! field's name in the collection, so the engine checks it against the
//! stored schema when the query runs, as it checks one made from a string.
//! What the types add is that the field is named through its type, which a
//! rename in the struct carries along, and that a value of another type
//! than the field's does not compile. A [`Query`] is not tied to a type, so
//! a condition on another collection's fields still fails only when it runs.

use std::fmt;
use std::marker::PhantomData;
use std::time::SystemTime;

use super::aggregate::Aggregate;
use super::build::{Filter, IntoPath, Query};
use super::ir::{Expr, Op};
use super::sealed;
use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::{micros_of, time_of};
use crate::schema::{
    CollectionType, ElementType, EmbeddedType, FieldOf, KeyType, Link, TypedReader, TypedWriter,
};

/// A path from the type `T` through embedded objects and links to a field
/// whose values are of type `V`, which [`FieldOf::then`] makes:
/// `Person::ADDRESS.then(Address::CITY)`, or `Post::AUTHOR.then(User::NAME)`
/// through a link. It has the methods of [`FieldOf`] that make conditions,
/// and a query sorts by it.
pub struct PathOf<T, V> {
    names: Vec<&'static str>,
    types: PhantomData<fn(&T) -> V>,
}

impl<T, V> PathOf<T, V> {
    /// The names of the fields the path goes through, in the collection,
    /// the last one the field it ends at.
    #[must_use]
    pub fn names(&self) -> &[&'static str] {
        &self.names
    }

    fn path(&self) -> Vec<String> {
        self.names.iter().map(|&name| name.to_owned()).collect()
    }
}

impl<T, V> From<FieldOf<T, V>> for PathOf<T, V> {
    fn from(field: FieldOf<T, V>) -> Self {
        Self {
            names: vec![field.name()],
            types: PhantomData,
        }
    }
}

impl<T, V> Clone for PathOf<T, V> {
    fn clone(&self) -> Self {
        Self {
            names: self.names.clone(),
            types: PhantomData,
        }
    }
}

impl<T, V> PartialEq for PathOf<T, V> {
    fn eq(&self, other: &Self) -> bool {
        self.names == other.names
    }
}

impl<T, V> fmt::Debug for PathOf<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PathOf")
            .field(&self.names.join("."))
            .finish()
    }
}

impl<T, V> FieldOf<T, V> {
    fn path(self) -> Vec<String> {
        vec![self.name().to_owned()]
    }
}

/// A field type a condition compares with a value of its own:
/// [`eq`](FieldOf::eq), [`ne`](FieldOf::ne) and [`is_in`](FieldOf::is_in).
///
/// The crate implements it for `bool`, `i64`, `f64`, `String`, `Vec<u8>`
/// (bytes), [`Link<T>`], whose value is the linked object's primary key,
/// and `Option` of any of them, whose value is the inner type's: a test for
/// null is [`is_null`](FieldOf::is_null).
pub trait ComparableType: sealed::Typed {
    /// The type of the values a condition compares the field with.
    type Compared;

    #[doc(hidden)]
    fn compared(value: Self::Compared) -> Value;
}

/// A field type whose values have an order: [`lt`](FieldOf::lt),
/// [`le`](FieldOf::le), [`gt`](FieldOf::gt), [`ge`](FieldOf::ge) and
/// [`between`](FieldOf::between), and [`TypedReader::min`] and
/// [`TypedReader::max`].
///
/// The crate implements it for `i64`, `f64`, `String`, `Vec<u8>` (bytes),
/// and `Option` of any of them.
pub trait OrderedType: ComparableType {
    #[doc(hidden)]
    fn from_aggregate(value: Value) -> Result<Self::Compared>;
}

/// A number field type, whose values [`TypedReader::sum`] and
/// [`TypedReader::avg`] add up: `i64`, `f64`, and `Option` of either.
pub trait NumberType: OrderedType {
    /// The type of a sum: `i64` for an int field, `f64` for a float field.
    type Sum;

    #[doc(hidden)]
    fn sum(value: Value) -> Result<Self::Sum>;
}

/// A string field type, which [`starts_with`](FieldOf::starts_with),
/// [`ends_with`](FieldOf::ends_with) and [`contains`](FieldOf::contains)
/// test: `String` and `Option<String>`.
pub trait TextType: ComparableType<Compared = String> {}

/// A field type a path goes on through, with [`then`](FieldOf::then): an
/// embedded object type, a [`Link<T>`], which goes on to the fields of `T`,
/// the linked object, and `Option` of either.
pub trait NestedType: sealed::Typed {
    /// The type whose field constants name the next step of the path.
    type Fields;
}

/// A link field type, whose objects a condition on the collection linked to
/// tests with [`any`](FieldOf::any): [`Link<T>`], `Option<Link<T>>` and
/// `Vec<Link<T>>`.
pub trait LinkType: sealed::Typed {
    /// The type of the collection linked to.
    type Target: CollectionType;
}

/// The error for an aggregate whose value is not of the type of the field
/// it was asked of, which a [`FieldOf`] made by hand with the name of a
/// field of another type gives.
fn not_of_the_field(value: &Value) -> Error {
    Error::InvalidArgument {
        message: format!("the field holds values of another type than its Rust type: {value:?}"),
    }
}

macro_rules! scalar {
    ($type:ty, $variant:ident) => {
        impl sealed::Typed for $type {}

        impl ComparableType for $type {
            type Compared = $type;

            fn compared(value: $type) -> Value {
                Value::$variant(value)
            }
        }
    };
}

scalar!(bool, Bool);
scalar!(i64, Int);
scalar!(f64, Float);
scalar!(String, String);
scalar!(Vec<u8>, Bytes);

macro_rules! ordered {
    ($type:ty, $variant:ident) => {
        impl OrderedType for $type {
            fn from_aggregate(value: Value) -> Result<$type> {
                match value {
                    Value::$variant(value) => Ok(value),
                    value => Err(not_of_the_field(&value)),
                }
            }
        }
    };
}

ordered!(i64, Int);
ordered!(f64, Float);
ordered!(String, String);
ordered!(Vec<u8>, Bytes);

impl sealed::Typed for SystemTime {}

/// A time a condition compares with a date field. One beyond the range of a
/// date, about 292,000 years from 1970, compares as the nearest date there
/// is, which orders it as it is ordered against every date a field holds.
impl ComparableType for SystemTime {
    type Compared = SystemTime;

    fn compared(time: SystemTime) -> Value {
        Value::Date(micros_of(time).unwrap_or_else(|| {
            if time < std::time::UNIX_EPOCH {
                i64::MIN
            } else {
                i64::MAX
            }
        }))
    }
}

impl OrderedType for SystemTime {
    fn from_aggregate(value: Value) -> Result<SystemTime> {
        match value {
            Value::Date(micros) => time_of(micros).ok_or_else(|| Error::InvalidArgument {
                message: format!(
                    "the date {micros} microseconds from 1970 is outside what a `SystemTime` holds here"
                ),
            }),
            value => Err(not_of_the_field(&value)),
        }
    }
}

impl NumberType for i64 {
    type Sum = i64;

    fn sum(value: Value) -> Result<i64> {
        Self::from_aggregate(value)
    }
}

impl NumberType for f64 {
    type Sum = f64;

    fn sum(value: Value) -> Result<f64> {
        Self::from_aggregate(value)
    }
}

impl TextType for String {}
impl TextType for Option<String> {}

impl<V: sealed::Typed> sealed::Typed for Option<V> {}

impl<V: ComparableType> ComparableType for Option<V> {
    type Compared = V::Compared;

    fn compared(value: V::Compared) -> Value {
        V::compared(value)
    }
}

impl<V: OrderedType> OrderedType for Option<V> {
    fn from_aggregate(value: Value) -> Result<V::Compared> {
        V::from_aggregate(value)
    }
}

impl<V: NumberType> NumberType for Option<V> {
    type Sum = V::Sum;

    fn sum(value: Value) -> Result<V::Sum> {
        V::sum(value)
    }
}

impl<V: NestedType> NestedType for Option<V> {
    type Fields = V::Fields;
}

impl<V: LinkType> LinkType for Option<V> {
    type Target = V::Target;
}

impl<U: CollectionType> sealed::Typed for Link<U> {}

impl<U: CollectionType> ComparableType for Link<U> {
    type Compared = U::Key;

    fn compared(key: U::Key) -> Value {
        key.to_value()
    }
}

impl<U: CollectionType> NestedType for Link<U> {
    type Fields = U;
}

impl<U: CollectionType> LinkType for Link<U> {
    type Target = U;
}

impl<E: ElementType> sealed::Typed for Vec<E> {}

impl<U: CollectionType> LinkType for Vec<Link<U>> {
    type Target = U;
}

impl<E: EmbeddedType> sealed::Typed for E {}

impl<E: EmbeddedType> NestedType for E {
    type Fields = E;
}

/// The methods that make conditions, on [`FieldOf`] and [`PathOf`] alike:
/// `$name` is the type, and `$receiver` the type with its parameters.
macro_rules! conditions {
    ($name:ident, $receiver:ty) => {
        impl<T, V> $name<T, V> {
            /// The field is null: an optional field left out, or a path
            /// through a null link or embedded object, or a link to an
            /// object that is not there.
            #[must_use]
            pub fn is_null(self) -> Filter {
                Filter::at(Op::IsNull, self.path(), Vec::new())
            }

            /// The field is not null.
            #[must_use]
            pub fn is_not_null(self) -> Filter {
                !self.is_null()
            }
        }

        impl<T, V: ComparableType> $name<T, V> {
            /// The field equals `value`.
            #[must_use]
            pub fn eq(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Eq, self.path(), vec![V::compared(value.into())])
            }

            /// The field is not null and differs from `value`.
            #[must_use]
            pub fn ne(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Ne, self.path(), vec![V::compared(value.into())])
            }

            /// The field equals one of `values`.
            #[must_use]
            pub fn is_in<I>(self, values: I) -> Filter
            where
                I: IntoIterator,
                I::Item: Into<V::Compared>,
            {
                let values = values
                    .into_iter()
                    .map(|value| V::compared(value.into()))
                    .collect();

                Filter::at(Op::In, self.path(), values)
            }
        }

        impl<T, V: OrderedType> $name<T, V> {
            /// The field is less than `value`.
            #[must_use]
            pub fn lt(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Lt, self.path(), vec![V::compared(value.into())])
            }

            /// The field is at most `value`.
            #[must_use]
            pub fn le(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Le, self.path(), vec![V::compared(value.into())])
            }

            /// The field is greater than `value`.
            #[must_use]
            pub fn gt(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Gt, self.path(), vec![V::compared(value.into())])
            }

            /// The field is at least `value`.
            #[must_use]
            pub fn ge(self, value: impl Into<V::Compared>) -> Filter {
                Filter::at(Op::Ge, self.path(), vec![V::compared(value.into())])
            }

            /// The field lies from `low` to `high`, both included.
            #[must_use]
            pub fn between(
                self,
                low: impl Into<V::Compared>,
                high: impl Into<V::Compared>,
            ) -> Filter {
                let values = vec![V::compared(low.into()), V::compared(high.into())];

                Filter::at(Op::Between, self.path(), values)
            }
        }

        impl<T, V: TextType> $name<T, V> {
            /// The string contains `text`.
            #[must_use]
            pub fn contains(self, text: impl Into<String>) -> Filter {
                Filter::at(Op::Contains, self.path(), vec![Value::String(text.into())])
            }

            /// The string starts with `text`.
            #[must_use]
            pub fn starts_with(self, text: impl Into<String>) -> Filter {
                Filter::at(
                    Op::StartsWith,
                    self.path(),
                    vec![Value::String(text.into())],
                )
            }

            /// The string ends with `text`.
            #[must_use]
            pub fn ends_with(self, text: impl Into<String>) -> Filter {
                Filter::at(Op::EndsWith, self.path(), vec![Value::String(text.into())])
            }
        }

        impl<T, E: ComparableType + ElementType> $name<T, Vec<E>> {
            /// The list holds the element `element`.
            #[must_use]
            pub fn contains(self, element: impl Into<E::Compared>) -> Filter {
                Filter::at(Op::Contains, self.path(), vec![E::compared(element.into())])
            }
        }

        impl<T, V: NestedType> $name<T, V> {
            /// The path from this field on to `next`, a field of the
            /// embedded object or of the linked object:
            /// `Person::ADDRESS.then(Address::CITY)`.
            #[must_use]
            pub fn then<W>(self, next: FieldOf<V::Fields, W>) -> PathOf<T, W> {
                let mut names = self.into_names();

                names.push(next.name());

                PathOf {
                    names,
                    types: PhantomData,
                }
            }
        }

        impl<T: CollectionType, V: LinkType> $name<T, V> {
            /// Holds for an object of the collection linked to that some
            /// object of `T`'s collection links to through this field:
            /// `Post::AUTHOR.any()`, on a query of users, finds those with
            /// a post. It is [`Filter::linked`].
            #[must_use]
            pub fn any(self) -> Filter {
                Filter(Expr::Linked {
                    collection: T::COLLECTION.to_owned(),
                    path: self.path(),
                    filter: None,
                })
            }

            /// [`any`](Self::any), where the linking object also meets
            /// `filter`, a condition on the fields of `T`:
            /// `Post::AUTHOR.any_where(Post::YEAR.ge(2020))`. It is
            /// [`Filter::linked_where`].
            #[must_use]
            pub fn any_where(self, filter: Filter) -> Filter {
                Filter(Expr::Linked {
                    collection: T::COLLECTION.to_owned(),
                    path: self.path(),
                    filter: Some(Box::new(filter.0)),
                })
            }
        }

        impl<T, V> sealed::Path for $receiver {}

        impl<T, V> IntoPath for $receiver {
            fn into_path(self) -> Vec<String> {
                self.path()
            }
        }
    };
}

impl<T, V> FieldOf<T, V> {
    fn into_names(self) -> Vec<&'static str> {
        vec![self.name()]
    }
}

impl<T, V> PathOf<T, V> {
    fn into_names(self) -> Vec<&'static str> {
        self.names
    }
}

conditions!(FieldOf, FieldOf<T, V>);
conditions!(PathOf, PathOf<T, V>);

impl<T, V> sealed::Path for &PathOf<T, V> {}

impl<T, V> IntoPath for &PathOf<T, V> {
    fn into_path(self) -> Vec<String> {
        self.path()
    }
}

macro_rules! aggregates {
    ($reader:ident, $untyped:ident) => {
        impl<T: CollectionType> $reader<'_, T> {
            /// The sum of `field`, a number field of `T` or a path from
            /// one, over the objects `query` finds, after its offset and
            /// within its limit: exact for an int field, and the exact sum
            /// rounded once for a float field. A sum of no values is zero.
            ///
            /// # Errors
            ///
            /// As [`CollectionReader::sum`](crate::CollectionReader::sum)
            /// fails: a sum of ints outside the range of an `i64` fails
            /// with [`Error::InvalidArgument`].
            pub fn sum<V: NumberType>(
                &self,
                query: &Query,
                field: impl Into<PathOf<T, V>>,
            ) -> Result<V::Sum> {
                let path = field.into().path();
                let sum = self.$untyped().aggregate(query, Aggregate::Sum, &path)?;

                V::sum(sum)
            }

            /// The least value of `field` over the objects `query` finds,
            /// in the order a sort gives; `None` when there is none.
            ///
            /// # Errors
            ///
            /// As [`CollectionReader::min`](crate::CollectionReader::min)
            /// fails.
            pub fn min<V: OrderedType>(
                &self,
                query: &Query,
                field: impl Into<PathOf<T, V>>,
            ) -> Result<Option<V::Compared>> {
                let path = field.into().path();
                let least = self.$untyped().aggregate(query, Aggregate::Min, &path)?;

                if least.is_null() {
                    return Ok(None);
                }

                V::from_aggregate(least).map(Some)
            }

            /// The greatest value of `field`, as [`min`](Self::min) gives
            /// the least.
            ///
            /// # Errors
            ///
            /// As [`CollectionReader::max`](crate::CollectionReader::max)
            /// fails.
            pub fn max<V: OrderedType>(
                &self,
                query: &Query,
                field: impl Into<PathOf<T, V>>,
            ) -> Result<Option<V::Compared>> {
                let path = field.into().path();
                let greatest = self.$untyped().aggregate(query, Aggregate::Max, &path)?;

                if greatest.is_null() {
                    return Ok(None);
                }

                V::from_aggregate(greatest).map(Some)
            }

            /// The mean of `field`, a number field, over the objects
            /// `query` finds: the exact sum divided by the number of
            /// values, rounded once; `None` when there are no values.
            ///
            /// # Errors
            ///
            /// As [`CollectionReader::avg`](crate::CollectionReader::avg)
            /// fails.
            pub fn avg<V: NumberType>(
                &self,
                query: &Query,
                field: impl Into<PathOf<T, V>>,
            ) -> Result<Option<f64>> {
                let path = field.into().path();
                let mean = self.$untyped().aggregate(query, Aggregate::Avg, &path)?;

                match mean {
                    Value::Null => Ok(None),
                    Value::Float(mean) => Ok(Some(mean)),
                    mean => Err(not_of_the_field(&mean)),
                }
            }
        }
    };
}

aggregates!(TypedReader, untyped);
aggregates!(TypedWriter, untyped_ref);
