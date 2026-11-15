//! Records and changes a binding hands the engine: any bytes are stored or
//! refused, in a write transaction that is thrown away.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let Ok(mut txn) = darudb_fuzz::database().begin_write() else {
        return;
    };

    for name in ["people", "teams"] {
        let Ok(mut collection) = txn.collection(name) else {
            return;
        };
        let _ = collection.insert_record(bytes);
        let _ = collection.put_record(bytes);
        let _ = collection.update_record(1, bytes);
        let _ = collection.update_record("north", bytes);
    }
});
