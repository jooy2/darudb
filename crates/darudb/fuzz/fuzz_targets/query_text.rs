//! The query language: any text parses or is refused, and what parses runs.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Ok(query) = darudb::Query::parse(text, &[]) {
        darudb_fuzz::run(&query);
    }

    // A prepared query takes its values later.
    let _ = darudb::Query::prepare(text);
});
