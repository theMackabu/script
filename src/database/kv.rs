use crate::{prelude::*, structs::config::Config};
use pickledb::{PickleDb, PickleDbDumpPolicy};
use rhai::{plugin::*, FnNamespace};
use std::cell::RefCell;

fn load(path: String) -> Option<PickleDb> {
    let config = Config::new().set_path(&crate::Cli::parse().config).read();

    if !file_exists!(&path) {
        PickleDb::new(&path, PickleDbDumpPolicy::AutoDump, config.kv_serialization_method()?);
    }

    match PickleDb::load(path, PickleDbDumpPolicy::AutoDump, config.kv_serialization_method()?) {
        Ok(db) => Some(db),
        Err(_) => None,
    }
}

// add .iter() method

#[export_module]
pub mod kv_db {
    #[derive(Clone)]
    pub struct KV<'s> {
        pub db: &'s RefCell<PickleDb>,
    }

    pub fn load<'s>(path: String) -> KV<'s> {
        // add error handling with error messages
        let db = RefCell::new(super::load(path).unwrap());
        KV { db: Box::leak(Box::new(db)) }
    }

    #[rhai_fn(global, pure, return_raw)]
    pub fn set(conn: &mut KV, key: String, value: String) -> Result<(), Box<EvalAltResult>> {
        let mut db = conn.db.borrow_mut();
        match db.set(&key, &value) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global)]
    pub fn get(conn: KV, key: String) -> String {
        let db = conn.db.borrow();
        match db.get::<String>(&key) {
            Some(data) => data,
            None => string!(""),
        }
    }

    #[rhai_fn(global, pure)]
    pub fn del(conn: &mut KV, key: String) -> bool {
        let mut db = conn.db.borrow_mut();
        match db.rem(&key) {
            Ok(bool) => bool,
            Err(_) => false,
        }
    }

    #[rhai_fn(global)]
    pub fn exists(conn: KV, key: String) -> bool { conn.db.borrow().exists(&key) }

    #[rhai_fn(global)]
    pub fn list(conn: KV) -> Vec<String> { conn.db.borrow().get_all() }

    #[rhai_fn(global)]
    pub fn count(conn: KV) -> i64 { conn.db.borrow().total_keys() as i64 }

    #[rhai_fn(global, name = "drop")]
    pub fn drop_db(conn: KV) { drop(conn.db.borrow()); }
}
