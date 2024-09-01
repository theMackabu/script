use crate::{config, helpers::collection_exists};

use rhai::{plugin::*, serde::to_dynamic, Array, FnNamespace};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use mongodb::{
    bson::{doc, Document},
    results::{CollectionSpecification, DeleteResult, InsertOneResult, UpdateResult},
    sync::{Client as MongoClient, Collection, Cursor, Database},
};

#[export_module]
pub mod mongo_db {
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
