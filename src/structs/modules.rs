use mongodb::sync::{Client as MongoClient, Database};
use redis::Client as RedisClient;
use rhai::Dynamic;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Http {
    pub length: Option<u64>,
    pub status: u16,
    pub err: Option<String>,
    pub body: Option<String>,
}

#[derive(Clone)]
pub struct Redis {
    pub client: Option<RedisClient>,
}

#[derive(Clone)]
pub struct Client {
    pub client: Option<MongoClient>,
}

#[derive(Clone)]
pub struct Mongo {
    pub db: Option<Database>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MongoDynamic(pub Dynamic);

unsafe impl Send for MongoDynamic {}
unsafe impl Sync for MongoDynamic {}
