use crate::{config, structs::modules::*};
use macros_rs::fmt::string;
use redis::{Client as RedisClient, Commands};
use rhai::{plugin::*, serde::to_dynamic, FnNamespace};
use std::collections::BTreeMap;

#[export_module]
pub mod redis_db {
    pub fn connect() -> Redis {
        let config = config::read().database.unwrap();
        match RedisClient::open(config.redis.unwrap().server) {
            Ok(client) => Redis { client: Some(client) },
            Err(_) => Redis { client: None },
        }
    }

    #[rhai_fn(global, return_raw, name = "set")]
    pub fn set_string(redis: Redis, key: String, value: String) -> Result<(), Box<EvalAltResult>> {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.set::<String, String, ()>(key, value) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "set")]
    pub fn set_i64(redis: Redis, key: String, value: i64) -> Result<(), Box<EvalAltResult>> {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.set::<String, i64, ()>(key, value) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, name = "get")]
    pub fn get(redis: Redis, key: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.get::<String, String>(key) {
            Ok(data) => data,
            Err(_) => string!(""),
        }
    }

    #[rhai_fn(global)]
    pub fn del(redis: Redis, key: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.del(key) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn expire(redis: Redis, key: String, s: i64) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.expire(key, s) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn persist(redis: Redis, key: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.persist(key) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn ttl(redis: Redis, key: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.ttl(key) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn rename(redis: Redis, key: String, new: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.rename(key, new) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn append(redis: Redis, key: String, value: String) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.append(key, value) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn inc(redis: Redis, key: String, value: i64) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.incr(key, value) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn dec(redis: Redis, key: String, value: i64) -> String {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.decr(key, value) {
            Ok(data) => data,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global)]
    pub fn exists(redis: Redis, key: String) -> bool {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.exists(key) {
            Ok(bool) => bool,
            Err(_) => false,
        }
    }

    #[rhai_fn(global, return_raw)]
    pub fn keys(redis: Redis, filter: String) -> Result<Dynamic, Box<EvalAltResult>> {
        let mut conn = redis.client.unwrap().get_connection().unwrap();
        match conn.keys(filter) {
            Ok(data) => to_dynamic::<Vec<String>>(data),
            Err(_) => to_dynamic::<Vec<String>>(vec![]),
        }
    }

    #[rhai_fn(global, return_raw, name = "list")]
    pub fn list_all(redis: Redis) -> Result<Dynamic, Box<EvalAltResult>> {
        let mut conn = redis.client.unwrap().get_connection().unwrap();

        let keys = match conn.keys("*") {
            Ok(data) => data,
            Err(_) => vec![],
        };

        let items = keys
            .into_iter()
            .map(|key| {
                let value: Option<String> = conn.get(&key).unwrap();
                (key, value.unwrap_or_else(|| "".to_string()))
            })
            .collect::<BTreeMap<String, String>>();

        to_dynamic(items)
    }

    #[rhai_fn(global, return_raw, name = "list")]
    pub fn list_filter(redis: Redis, filter: String) -> Result<Dynamic, Box<EvalAltResult>> {
        let mut conn = redis.client.unwrap().get_connection().unwrap();

        let keys = match conn.keys(filter) {
            Ok(data) => data,
            Err(_) => vec![],
        };

        let items = keys
            .into_iter()
            .map(|key| {
                let value: Option<String> = conn.get(&key).unwrap();
                (key, value.unwrap_or_else(|| "".to_string()))
            })
            .collect::<BTreeMap<String, String>>();

        to_dynamic(items)
    }
}
