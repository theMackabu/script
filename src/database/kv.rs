use macros_rs::string;
use pickledb::{PickleDb, PickleDbDumpPolicy, SerializationMethod};

use crate::config::{
    self,
    structs::{Database, KV},
};

pub fn load(path: String) -> PickleDb {
    let config = config::read().database.unwrap_or(Database {
        kv: Some(KV { method: string!("default") }),
        mongo: None,
        sqlite: None,
    });

    let method = match config.kv.unwrap().method.as_str() {
        "json" | "default" => SerializationMethod::Json,
        "yaml" | "yml" => SerializationMethod::Yaml,
        "cbor" | "conbin" => SerializationMethod::Bin,
        "binary" | "bin" => SerializationMethod::Bin,
        _ => SerializationMethod::Bin,
    };

    PickleDb::new(path, PickleDbDumpPolicy::AutoDump, method)
}
