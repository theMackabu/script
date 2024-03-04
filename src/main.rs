mod config;
mod database;
mod file;
mod helpers;

use askama::Template;
use config::structs::Config;
use lazy_static::lazy_static;
use mime::Mime;
use pickledb::PickleDb;
use redis::{Client as RedisClient, Commands};
use regex::{Captures, Error, Regex};
use reqwest::blocking::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use smartstring::alias::String as SmString;
use std::{cell::RefCell, collections::BTreeMap, env, fs, sync::Arc};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{filter::LevelFilter, prelude::*};

use rhai::{packages::Package, plugin::*, serde::to_dynamic, Array, Dynamic, Engine, FnNamespace, Map, Scope};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use mongodb::{
    bson::{doc, Document},
    results::{CollectionSpecification, DeleteResult, InsertOneResult, UpdateResult},
    sync::{Client as MongoClient, Collection, Cursor, Database},
};

use macros_rs::{
    exp::ternary,
    fmt::{crashln, str, string},
};

use actix_web::{
    http::{header::ContentType, StatusCode},
    web::{self, Data},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};

#[derive(Template)]
#[template(path = "error.html")]
struct ServerError {
    error: String,
    context: Vec<(String, String)>,
}

#[derive(Template)]
#[template(path = "message.html")]
struct Message<'a> {
    code: u16,
    note: &'a str,
    error: &'a str,
    message: String,
}

// convert to peg
lazy_static! {
    static ref R_INDEX: Result<Regex, Error> = Regex::new(r"index\s*\{");
    static ref R_ERR: Result<Regex, Error> = Regex::new(r"(\b\d{3})\s*\{");
    static ref R_FN: Result<Regex, Error> = Regex::new(r"(\w+)\((.*?)\)\s*\{");
    static ref R_DOT: Result<Regex, Error> = Regex::new(r"\.(\w+)\((.*?)\)\s*\{");
    static ref R_WILD: Result<Regex, Error> = Regex::new(r"\*\s*\{|wildcard\s*\{");
    static ref R_SLASH: Result<Regex, Error> = Regex::new(r"(?m)\/(?=.*\((.*?)\)\s*\{[^{]*$)");
}

pub fn response(data: String, content_type: String, status_code: i64) -> (String, ContentType, StatusCode) {
    let content_type = match content_type.as_str() {
        "xml" => ContentType::xml(),
        "png" => ContentType::png(),
        "html" => ContentType::html(),
        "json" => ContentType::json(),
        "jpeg" => ContentType::jpeg(),
        "text" => ContentType::plaintext(),
        "stream" => ContentType::octet_stream(),
        "form" => ContentType::form_url_encoded(),
        _ => ContentType::plaintext(),
    };

    (data, content_type, helpers::convert_status(status_code))
}

pub fn proxy(url: String) -> (String, ContentType, StatusCode) {
    let client = ReqwestClient::new();
    let response = match client.get(url).send() {
        Ok(res) => res,
        Err(err) => return (err.to_string(), ContentType::plaintext(), StatusCode::GATEWAY_TIMEOUT),
    };

    let status = response.status();
    let content_type = response.headers().get("Content-Type").unwrap().to_str().unwrap_or("text/plain").parse::<Mime>().unwrap();

    if status.is_success() {
        (response.text().unwrap(), ContentType(content_type), status)
    } else {
        (response.text().unwrap(), ContentType(content_type), status)
    }
}

fn match_route(route_template: &str, placeholders: &[&str], url: &str) -> Option<Vec<String>> {
    let mut matched_placeholders = Vec::new();

    let route_segments: Vec<&str> = route_template.split('/').collect();
    let url_segments: Vec<&str> = url.split('/').collect();

    if route_segments.len() != url_segments.len() {
        return None;
    }

    for (route_segment, url_segment) in route_segments.iter().zip(url_segments.iter()) {
        if let Some(placeholder_value) = match_segment(route_segment, url_segment, placeholders) {
            if !placeholder_value.is_empty() {
                matched_placeholders.push(placeholder_value);
            }
        } else {
            return None;
        }
    }

    Some(matched_placeholders)
}

fn collection_exists(d: &Database, name: &String) -> Result<bool, Box<EvalAltResult>> {
    let filter = doc! { "name": &name };

    match d.list_collection_names(Some(filter)) {
        Err(err) => Err(err.to_string().into()),
        Ok(list) => Ok(list.into_iter().any(|col| col == *name)),
    }
}

fn match_segment(route_segment: &str, url_segment: &str, placeholders: &[&str]) -> Option<String> {
    if route_segment.starts_with('{') && route_segment.ends_with('}') {
        let placeholder = &route_segment[1..route_segment.len() - 1];
        if placeholders.contains(&placeholder) {
            Some(url_segment.to_string())
        } else {
            None
        }
    } else if route_segment == url_segment {
        Some("".to_string())
    } else {
        let route_parts: Vec<&str> = route_segment.split('.').collect();
        let url_parts: Vec<&str> = url_segment.split('.').collect();
        if route_parts.len() == url_parts.len() && route_parts.last() == url_parts.last() {
            match_segment(route_parts[0], url_parts[0], placeholders)
        } else {
            None
        }
    }
}

#[export_module]
mod default {
    pub fn text(string: String) -> (String, ContentType, StatusCode) { (string, ContentType::plaintext(), StatusCode::OK) }
    pub fn html(string: String) -> (String, ContentType, StatusCode) { (string, ContentType::html(), StatusCode::OK) }
    pub fn json(object: Dynamic) -> (String, ContentType, StatusCode) {
        match serde_json::to_string(&object) {
            Ok(result) => (result, ContentType::json(), StatusCode::OK),
            Err(err) => (err.to_string(), ContentType::plaintext(), StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[export_module]
mod status {
    pub fn text(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::plaintext(), helpers::convert_status(status)) }
    pub fn html(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::html(), helpers::convert_status(status)) }
    pub fn json(object: Dynamic, status: i64) -> (String, ContentType, StatusCode) {
        match serde_json::to_string(&object) {
            Ok(result) => (result, ContentType::json(), helpers::convert_status(status)),
            Err(err) => (err.to_string(), ContentType::plaintext(), StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[export_module]
mod json {
    pub fn dump<'s>(object: Dynamic) -> String {
        match serde_json::to_string(&object) {
            Ok(result) => result,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global, return_raw, name = "parse")]
    pub fn parse<'s>(json: String) -> Result<Map, Box<EvalAltResult>> {
        match serde_json::from_str(&json) {
            Ok(map) => Ok(map),
            Err(err) => Err(err.to_string().into()),
        }
    }
}

#[export_module]
mod mongo_db {
    #[derive(Clone)]
    pub struct Client {
        pub client: Option<MongoClient>,
    }

    #[derive(Clone)]
    pub struct Mongo {
        pub db: Option<Database>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct MongoDynamic(Dynamic);

    unsafe impl Send for MongoDynamic {}
    unsafe impl Sync for MongoDynamic {}

    trait MongoVec {
        fn into_vec(self) -> Vec<MongoDynamic>;
    }

    trait MongoDocument {
        fn into_doc(self) -> Document;
        fn into_map(self) -> MongoDynamic;
    }

    impl Into<MongoDynamic> for Dynamic {
        fn into(self) -> MongoDynamic { MongoDynamic(self) }
    }

    impl FromIterator<MongoDynamic> for Array {
        fn from_iter<I: IntoIterator<Item = MongoDynamic>>(iter: I) -> Self { iter.into_iter().map(|m| m.0).collect() }
    }

    impl MongoVec for Array {
        fn into_vec(self) -> Vec<MongoDynamic> { self.into_iter().map(|m| m.into_map()).collect() }
    }

    impl MongoDocument for Dynamic {
        fn into_doc(self) -> Document {
            Document::from(
                serde_json::from_str(&match serde_json::to_string(&self) {
                    Ok(data) => data,
                    Err(err) => format!("{{\"err\": \"{err}\"}}"),
                })
                .unwrap_or(doc! {"err": format!("unable to deserialize {self}")}),
            )
        }
        fn into_map(self) -> MongoDynamic { MongoDynamic(to_dynamic(self.into_doc()).unwrap()) }
    }

    pub fn connect() -> Client {
        let config = config::read().database.unwrap();
        match MongoClient::with_uri_str(config.mongo.unwrap().server.unwrap_or("".to_string())) {
            Ok(client) => Client { client: Some(client) },
            Err(_) => Client { client: None },
        }
    }

    pub fn shutdown(conn: Client) { conn.client.unwrap().shutdown(); }

    #[rhai_fn(global, return_raw, name = "list")]
    pub fn list_databases(conn: Client) -> Result<Dynamic, Box<EvalAltResult>> {
        match conn.client {
            Some(client) => match client.list_databases(None, None) {
                Err(err) => Err(err.to_string().into()),
                Ok(list) => to_dynamic(list),
            },
            None => to_dynamic::<Array>(vec![]),
        }
    }

    #[rhai_fn(global, return_raw, name = "count")]
    pub fn count_databases(conn: Client) -> Result<i64, Box<EvalAltResult>> {
        match conn.client {
            Some(client) => match client.list_databases(None, None) {
                Err(err) => Err(err.to_string().into()),
                Ok(list) => Ok(list.len() as i64),
            },
            None => Ok(0),
        }
    }

    #[rhai_fn(global)]
    pub fn db(conn: Client, name: String) -> Mongo {
        match conn.client {
            None => Mongo { db: None },
            Some(client) => Mongo { db: Some(client.database(&name)) },
        }
    }

    #[rhai_fn(global, return_raw, name = "list")]
    pub fn list_collections(m: Mongo) -> Result<Dynamic, Box<EvalAltResult>> {
        match m.db {
            Some(client) => match client.list_collections(None, None) {
                Err(err) => Err(err.to_string().into()),
                Ok(list) => to_dynamic(list.map(|item| item.unwrap()).collect::<Vec<CollectionSpecification>>()),
            },
            None => to_dynamic::<Array>(vec![]),
        }
    }

    #[rhai_fn(global, return_raw, name = "get")]
    pub fn collection(m: Mongo, name: String) -> Result<Collection<MongoDynamic>, Box<EvalAltResult>> {
        match m.db {
            Some(client) => Ok(client.collection(&name)),
            None => Err("No database found".into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "create")]
    pub fn create_collection(m: Mongo, name: String) -> Result<Collection<MongoDynamic>, Box<EvalAltResult>> {
        match m.db {
            Some(client) => match collection_exists(&client, &name).unwrap() {
                true => Ok(client.collection(&name)),
                false => match client.create_collection(&name, None) {
                    Err(err) => Err(err.to_string().into()),
                    Ok(_) => Ok(client.collection(&name)),
                },
            },
            None => Err("No database found".into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "count")]
    pub fn count_collections(collection: Collection<MongoDynamic>) -> Result<i64, Box<EvalAltResult>> {
        match collection.count_documents(None, None) {
            Ok(count) => Ok(count as i64),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "find")]
    pub fn find_all(collection: Collection<MongoDynamic>) -> Result<Arc<Cursor<MongoDynamic>>, Box<EvalAltResult>> {
        match collection.find(None, None) {
            Ok(cursor) => Ok(Arc::new(cursor)),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "find_one")]
    pub fn find_one(collection: Collection<MongoDynamic>, filter: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        match collection.find_one(filter.into_doc(), None) {
            Ok(cursor) => match cursor {
                Some(item) => to_dynamic::<MongoDynamic>(item),
                None => to_dynamic::<Vec<MongoDynamic>>(vec![]),
            },
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "find")]
    pub fn find_filter(collection: Collection<MongoDynamic>, filter: Dynamic) -> Result<Arc<Cursor<MongoDynamic>>, Box<EvalAltResult>> {
        match collection.find(filter.into_doc(), None) {
            Ok(cursor) => Ok(Arc::new(cursor)),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, name = "count")]
    pub fn count_cursor(cursor: Arc<Cursor<MongoDynamic>>) -> i64 {
        match Arc::into_inner(cursor) {
            Some(cursor) => cursor.count() as i64,
            None => 0,
        }
    }

    #[rhai_fn(global, name = "count")]
    pub fn count_collect(items: Array) -> i64 { items.iter().count() as i64 }

    #[rhai_fn(global, return_raw, name = "collect")]
    pub fn collect(cursor: Arc<Cursor<MongoDynamic>>) -> Result<Dynamic, Box<EvalAltResult>> {
        match Arc::into_inner(cursor) {
            Some(cursor) => match cursor.collect() {
                Ok(items) => to_dynamic::<Array>(items),
                Err(err) => Err(err.to_string().into()),
            },
            None => to_dynamic::<Array>(vec![]),
        }
    }

    #[rhai_fn(global, name = "drop")]
    pub fn drop_collection(collection: Collection<MongoDynamic>) -> bool {
        match collection.drop(None) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[rhai_fn(global, return_raw, name = "drop")]
    pub fn drop_database(m: Mongo) -> Result<bool, Box<EvalAltResult>> {
        match m.db {
            Some(client) => match client.drop(None) {
                Ok(_) => Ok(true),
                Err(err) => Err(err.to_string().into()),
            },
            None => Err("No collection found".into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "insert")]
    pub fn insert_one(collection: Collection<MongoDynamic>, map: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        match collection.insert_one(map.into_map(), None) {
            Ok(res) => to_dynamic::<InsertOneResult>(res),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "insert")]
    pub fn insert_many(collection: Collection<MongoDynamic>, map: Array) -> Result<Array, Box<EvalAltResult>> {
        match collection.insert_many(map.into_vec(), None) {
            Ok(res) => Ok(res.inserted_ids.into_iter().map(|(_, value)| to_dynamic(value).unwrap()).collect::<Array>()),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw)]
    pub fn delete(collection: Collection<MongoDynamic>, map: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        match collection.delete_one(map.into_doc(), None) {
            Ok(res) => to_dynamic::<DeleteResult>(res),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw)]
    pub fn delete_many(collection: Collection<MongoDynamic>, map: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        match collection.delete_many(map.into_doc(), None) {
            Ok(res) => to_dynamic::<DeleteResult>(res),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(global, return_raw)]
    pub fn update(collection: Collection<MongoDynamic>, query: Dynamic, replacement: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
        let replacement: MongoDynamic = replacement.into();
        match collection.replace_one(query.into_doc(), replacement, None) {
            Ok(res) => to_dynamic::<UpdateResult>(res),
            Err(err) => Err(err.to_string().into()),
        }
    }
}

#[export_module]
mod kv_db {
    #[derive(Clone)]
    pub struct KV<'s> {
        pub db: &'s RefCell<PickleDb>,
    }

    pub fn load<'s>(path: String) -> KV<'s> {
        let db = RefCell::new(database::kv::load(path));
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

#[export_module]
mod redis_db {
    #[derive(Clone)]
    pub struct Redis {
        pub client: Option<RedisClient>,
    }

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

#[export_module]
mod http {
    #[derive(Clone)]
    pub struct Http {
        pub length: Option<u64>,
        pub status: u16,
        pub err: Option<String>,
        pub body: Option<String>,
    }

    impl From<Http> for Map {
        fn from(http: Http) -> Self {
            let mut map = Map::new();
            map.insert(SmString::from("status"), Dynamic::from(http.status as i64));

            if let Some(length) = http.length {
                map.insert(SmString::from("length"), Dynamic::from(length as i64));
            }
            if let Some(err) = http.err {
                map.insert(SmString::from("err"), Dynamic::from(err));
            }
            if let Some(body) = http.body {
                map.insert(SmString::from("body"), Dynamic::from(body));
            }

            return map;
        }
    }

    pub fn get(url: String) -> Http {
        let client = ReqwestClient::new();
        let response = match client.get(url).send() {
            Ok(res) => res,
            Err(err) => {
                return Http {
                    length: Some(0),
                    status: 0,
                    err: Some(err.to_string()),
                    body: None,
                }
            }
        };

        if response.status().is_success() {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: None,
                body: Some(response.text().unwrap()),
            }
        } else {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: Some(response.text().unwrap()),
                body: None,
            }
        }
    }

    pub fn post(url: String, data: Map) -> Http {
        let client = ReqwestClient::new();

        let data = match serde_json::to_string(&data) {
            Ok(result) => result,
            Err(err) => err.to_string(),
        };

        let response = match client.post(url).body(data).send() {
            Ok(res) => res,
            Err(err) => {
                return Http {
                    length: Some(0),
                    status: 0,
                    err: Some(err.to_string()),
                    body: None,
                }
            }
        };

        if response.status().is_success() {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: None,
                body: Some(response.text().unwrap()),
            }
        } else {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: Some(response.text().unwrap()),
                body: None,
            }
        }
    }

    #[rhai_fn(get = "status")]
    pub fn status(res: Http) -> i64 { res.status as i64 }

    #[rhai_fn(global, pure, return_raw, name = "raw")]
    pub fn raw(res: &mut Http) -> Result<Map, Box<EvalAltResult>> { Ok(res.clone().into()) }

    #[rhai_fn(get = "length")]
    pub fn length(res: Http) -> i64 {
        match res.length {
            Some(len) => len as i64,
            None => 0,
        }
    }

    #[rhai_fn(get = "body", return_raw)]
    pub fn body(res: Http) -> Result<String, Box<EvalAltResult>> {
        match res.body {
            Some(body) => Ok(body.to_string()),
            None => Ok(string!("")),
        }
    }

    #[rhai_fn(global, pure, return_raw, name = "json")]
    pub fn json(res: &mut Http) -> Result<Map, Box<EvalAltResult>> {
        let body = str!(res.body.clone().unwrap());
        match serde_json::from_str(body) {
            Ok(map) => Ok(map),
            Err(err) => Err(err.to_string().into()),
        }
    }

    #[rhai_fn(get = "error", return_raw)]
    pub fn error(res: Http) -> Result<Map, Box<EvalAltResult>> {
        let err = match res.err {
            Some(err) => format!("\"{err}\""),
            None => string!("null"),
        };
        match serde_json::from_str(&format!("{{\"message\":{err}}}")) {
            Ok(msg) => Ok(msg),
            Err(err) => Err(err.to_string().into()),
        }
    }
}

async fn handler(req: HttpRequest, config: Data<Config>) -> impl Responder {
    let url = match req.uri().to_string().strip_prefix("/") {
        Some(url) => url.to_string(),
        None => req.uri().to_string(),
    };

    macro_rules! send {
        ($response:expr) => {{
            let (body, content_type, status_code) = $response;
            tracing::info!(
                method = string!(req.method()),
                status = string!(status_code),
                content = string!(content_type),
                "request '{}'",
                req.uri()
            );
            return HttpResponse::build(status_code).content_type(content_type).body(body);
        }};
    }

    if url.as_str() == "favicon.ico" {
        return HttpResponse::Ok().body("");
    }

    let mut routes: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let filename = &config.workers.get(0).unwrap();
    let fs_pkg = FilesystemPackage::new();
    let url_pkg = UrlPackage::new();

    let json = exported_module!(json);
    let http = exported_module!(http);
    let exists = exported_module!(file::exists);

    let mut engine = Engine::new();
    let mut scope = Scope::new();

    let path = match url.as_str() {
        "" => "_route_index".to_string(),
        _ => helpers::convert_to_format(&url.to_string()),
    };

    fs_pkg.register_into_engine(&mut engine);
    url_pkg.register_into_engine(&mut engine);

    engine.register_static_module("json", json.into());
    engine.register_static_module("http", http.into());
    engine.register_static_module("exists", exists.into());

    if let Some(database) = &config.database {
        if let Some(_) = &database.kv {
            let kv = exported_module!(kv_db);
            engine.register_static_module("kv", kv.into());
        }
        if let Some(_) = &database.mongo {
            let mongo = exported_module!(mongo_db);
            engine.register_static_module("mongo", mongo.into());
        }
        if let Some(_) = &database.redis {
            let redis = exported_module!(redis_db);
            engine.register_static_module("redis", redis.into());
        }
    }

    #[derive(Clone)]
    struct Request {
        path: String,
        url: String,
        version: String,
        query: String,
    }

    impl Request {
        fn to_dynamic(&self) -> Dynamic {
            let mut map = Map::new();

            map.insert(SmString::from("path"), Dynamic::from(self.path.clone()));
            map.insert(SmString::from("url"), Dynamic::from(self.url.clone()));
            map.insert(SmString::from("version"), Dynamic::from(self.version.clone()));
            map.insert(SmString::from("query"), Dynamic::from(self.query.clone()));

            Dynamic::from(map)
        }
    }

    let request = Request {
        path: url.to_string(),
        url: req.uri().to_string(),
        version: format!("{:?}", req.version()),
        query: req.query_string().to_string(),
    };

    scope.push("request", request.to_dynamic());

    engine
        .register_fn("proxy", proxy)
        .register_fn("cwd", file::cwd)
        .register_fn("response", response)
        .register_fn("text", default::text)
        .register_fn("json", default::json)
        .register_fn("html", default::html)
        .register_fn("text", status::text)
        .register_fn("json", status::json)
        .register_fn("html", status::html);

    let contents = match fs::read_to_string(&filename) {
        Ok(contents) => contents,
        Err(err) => crashln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err),
    };

    let has_error_page = R_ERR.as_ref().unwrap().is_match(&contents).unwrap();
    let has_wildcard = R_WILD.as_ref().unwrap().is_match(&contents).unwrap();
    let has_index = R_INDEX.as_ref().unwrap().is_match(&contents).unwrap();

    let contents = {
        let pattern = r#"\{([^{}\s]+)\}"#;
        let pattern_combine = r#"(?m)^_route/(.*)\n(.*?)\((.*?)\)"#;

        let re = Regex::new(pattern).unwrap();
        let re_combine = Regex::new(pattern_combine).unwrap();

        let result = re.replace_all(&contents, |captures: &regex::Captures| {
            let content = captures.get(1).map_or("", |m| m.as_str());
            format!("_arg_{content}")
        });

        let output = result.replace("#[route(\"", "_route").replace("\")]", "");

        re_combine.replace_all(str!(output), |captures: &regex::Captures| {
            let path = captures.get(1).map_or("", |m| m.as_str());
            let args = captures.get(3).map_or("", |m| m.as_str());

            if args != "" {
                let r_path = Regex::new(r"(?m)_arg_(\w+)").unwrap();
                let key = r_path.replace_all(&path, |captures: &regex::Captures| {
                    let key = captures.get(1).map_or("", |m| m.as_str());
                    format!("{{{key}}}")
                });

                routes.insert(string!(key), args.split(",").map(|s| s.to_string().replace(" ", "")).collect());
                format!("fmt_{path}({args})")
            } else {
                routes.insert(string!(path), vec![]);
                format!("{path}()")
            }
        })
    };

    // cache contents until file change
    let contents = {
        let result = R_SLASH.as_ref().unwrap().replace_all(&contents, "_").to_string();
        let result = R_INDEX.as_ref().unwrap().replace_all(&result, "index() {").to_string();
        let result = R_ERR.as_ref().unwrap().replace_all(&result, "error_$1() {").to_string();

        let pattern_route = r#"(?m)^(?!_error)(?!_wildcard)(?!_index)(?!fmt_)(.*?)\((.*?)\)\s*\{"#;
        let re_route = Regex::new(pattern_route).unwrap();

        for captures in re_route.captures_iter(&contents) {
            let path = captures.unwrap().get(1).map_or("", |m| m.as_str());
            routes.insert(string!(path.replace("_", "/")), vec![]);
        }

        let result = R_DOT
            .as_ref()
            .unwrap()
            .replace_all(&result, |captures: &Captures| format!("__d{}", helpers::rm_first(&captures[0])))
            .to_string();
        let result = R_FN.as_ref().unwrap().replace_all(&result, |captures: &Captures| format!("fn _route_{}", &captures[0])).to_string();

        ternary!(has_wildcard, R_WILD.as_ref().unwrap().replace_all(&result, "fn _wildcard() {").to_string(), result)
    };

    let contents = {
        let slash = Regex::new(r"%\((.*?)\)").unwrap();
        slash.replace_all(&contents, |caps: &regex::Captures| format!("${{{}}}", &caps[1])).to_string()
    };

    let mut ast = match engine.compile(&contents) {
        Ok(ast) => ast,
        Err(err) => helpers::error(&engine, &path, err),
    };

    ast.set_source(filename.to_string_lossy().to_string());

    if url.as_str() == "" && has_index {
        send!(engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, "_route_index", ()).unwrap());
    };

    fn extract_context(contents: String, err: String) -> Vec<(String, String)> {
        let re = Regex::new(r"line (\d+)").unwrap();

        if let Some(captures) = re.captures(&err).unwrap() {
            if let Some(num) = captures.get(1) {
                if let Ok(line_number) = num.as_str().parse::<usize>() {
                    let lines: Vec<&str> = contents.lines().collect();
                    let start_line = line_number.saturating_sub(3);
                    let end_line = (line_number + 4).min(lines.len());

                    return lines[start_line..end_line]
                        .iter()
                        .enumerate()
                        .map(|(i, line)| (format!("{:>4}", start_line + i + 1), line.to_string()))
                        .collect::<Vec<(String, String)>>();
                }
            }
        }

        vec![]
    }

    for (route, args) in routes {
        let url = url.clone();
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        if url.as_str() == route {
            match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, helpers::convert_to_format(&url.to_string()), ()) {
                Ok(response) => send!(response),
                Err(err) => {
                    let body = ServerError {
                        error: err.to_string().replace("\n", "<br>"),
                        context: extract_context(contents, err.to_string()),
                    };

                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).content_type(ContentType::html()).body(body.render().unwrap());
                }
            }
        }

        match match_route(&route, &args, url.as_str()) {
            Some(data) => match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, helpers::route_to_fn(&route), data) {
                Ok(response) => send!(response),
                Err(err) => {
                    let body = ServerError {
                        error: err.to_string().replace("\n", "<br>"),
                        context: extract_context(contents, err.to_string()),
                    };

                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).content_type(ContentType::html()).body(body.render().unwrap());
                }
            },
            None => {}
        }
    }

    if has_wildcard || has_error_page {
        let (body, content_type, status_code) = engine
            .call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, ternary!(has_wildcard, "_wildcard", "_route_error_404"), ())
            .unwrap();

        send!((body, content_type, ternary!(has_wildcard, status_code, StatusCode::NOT_FOUND)))
    } else {
        let body = Message {
            error: "Function Not Found",
            code: StatusCode::NOT_FOUND.as_u16(),
            message: format!("Have you created the <code>{url}()</code> route?"),
            note: "You can add <code>* {}</code> or <code>404 {}</code> routes as well",
        };

        send!((body.render().unwrap(), ContentType::html(), StatusCode::NOT_FOUND))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env::set_var("RUST_LOG", "INFO");

    let config = config::read();
    let app = || App::new().app_data(Data::new(config::read())).default_service(web::to(handler));

    let formatting_layer = BunyanFormattingLayer::new("server".into(), std::io::stdout)
        .skip_fields(vec!["file", "line"].into_iter())
        .expect("Unable to create logger");

    tracing_subscriber::registry()
        .with(LevelFilter::from(tracing::Level::INFO))
        .with(JsonStorageLayer)
        .with(formatting_layer)
        .init();

    tracing::info!(address = config.settings.address, port = config.settings.port, "server started");
    HttpServer::new(app).bind(config.get_address()).unwrap().run().await
}
