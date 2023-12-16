use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use toml::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub workers: Vec<PathBuf>,
    pub settings: Settings,
    pub database: Option<Database>,
    pub env: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub address: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Database {
    pub kv: Option<KV>,
    pub sqlite: Option<SQlite>,
    pub mongo: Option<Mongo>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KV {
    pub method: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SQlite {
    pub connect: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mongo {
    pub database: String,
    pub address: String,
    pub port: u64,
    pub auth: MongoAuth,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MongoAuth {
    pub username: String,
    pub password: String,
}
