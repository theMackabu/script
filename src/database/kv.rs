use crate::config;
use pickledb::{PickleDb, PickleDbDumpPolicy, SerializationMethod};

pub fn load(path: String) -> PickleDb {
    let config = config::read().database.unwrap();
    let method = match config.kv.unwrap().method.as_str() {
        "json" | "default" => SerializationMethod::Json,
        "yaml" | "yml" => SerializationMethod::Yaml,
        "cbor" | "conbin" => SerializationMethod::Bin,
        "binary" | "bin" => SerializationMethod::Bin,
        _ => SerializationMethod::Bin,
    };

    PickleDb::new(path, PickleDbDumpPolicy::AutoDump, method)
}

// use load to not erase db on every load
// add .iter() method
