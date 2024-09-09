use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use toml::Value;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip)]
    pub config_path: PathBuf,
    pub workers: Vec<PathBuf>,
    pub settings: Settings,
    pub database: Option<Database>,
    pub env: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub cache: String,
    pub address: String,
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Database {
    pub kv: Option<KVConfig>,
    pub mongo: Option<MongoConfig>,
    pub redis: Option<RedisConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KVConfig {
    pub method: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub server: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MongoConfig {
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
