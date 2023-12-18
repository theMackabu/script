use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use toml::Value;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub workers: Vec<PathBuf>,
    pub settings: Settings,
    pub database: Option<Database>,
    pub env: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub address: String,
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Database {
    pub kv: Option<KV>,
    pub mongo: Option<Mongo>,
    pub redis: Option<Redis>,
    pub sqlite: Option<SQlite>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KV {
    pub method: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SQlite {
    pub connect: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Redis {
    pub server: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Mongo {
    pub server: Option<String>,
    pub advanced: Option<MongoAdvanced>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MongoAdvanced {
    pub port: u64,
    pub address: String,
    pub auth: Option<MongoAuth>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MongoAuth {
    pub username: String,
    pub password: String,
}
