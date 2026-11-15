//! The IR a binding hands the engine: any bytes decode or are refused, and
//! what decodes is planned and run, whatever it names.
#![no_main]

use darudb::QueryRequest;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let Ok(request) = QueryRequest::decode(bytes) else {
        return;
    };

    // Against the people, whatever collection the request names.
    darudb_fuzz::run(&request.query);

    // And as a binding runs it: on the collection it names, for records, a
    // count or its aggregate.
    if let Ok(read) = darudb_fuzz::database().begin_read() {
        if let Ok(collection) = read.collection(&request.collection) {
            let _ = collection.query_records(&request.query);

            if let Some((function, path)) = &request.aggregate {
                let _ = collection.aggregate(&request.query, *function, path);
            }
        }
    }

    // What decodes encodes again.
    let _ = request.encode();
});
