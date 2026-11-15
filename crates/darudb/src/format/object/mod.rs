//! What the object layer's bytes mean: values, the key encoding, the record
//! encoding, the stored schema, the names of its trees, and the terms a text
//! index holds for a text. `design/objects.md` is the specification.
//!
//! Like the rest of `format`, this is pure functions over bytes, with no I/O,
//! so every rule about the encodings is tested without a database.

pub(crate) mod codec;
pub(crate) mod fold_table;
pub(crate) mod key;
pub(crate) mod names;
pub(crate) mod schema;
pub(crate) mod text;
mod value;
mod word_table;

pub use value::{Object, Value};
pub(crate) use value::{micros_of, time_of};
