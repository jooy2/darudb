//! What the object layer's bytes mean: values, the key encoding, the record
//! encoding, and the stored schema. `design/objects.md` is the
//! specification.
//!
//! Like the rest of `format`, this is pure functions over bytes, with no I/O,
//! so every rule about the encodings is tested without a database.

pub(crate) mod codec;
pub(crate) mod key;
pub(crate) mod schema;
mod value;

pub use value::{Object, Value};
