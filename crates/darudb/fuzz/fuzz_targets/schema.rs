//! The schema a binding encodes: any bytes decode or are refused.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = darudb::Schema::decode(bytes);
});
