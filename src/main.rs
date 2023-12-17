mod config;
mod database;
mod file;

use config::structs::Config;
use lazy_static::lazy_static;
use macros_rs::{crashln, str, string, ternary};
use pickledb::PickleDb;
use regex::{Captures, Error, Regex};
use reqwest::blocking::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use smartstring::alias::String as SmString;
use std::{cell::RefCell, collections::BTreeMap, env, fs, sync::Arc};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{filter::LevelFilter, prelude::*};

use rhai::{packages::Package, plugin::*, serde::to_dynamic, Dynamic, Engine, FnNamespace, Map, ParseError, Scope, AST};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use mongodb::{
    bson::Document,
    results::CollectionSpecification,
    sync::{Client as MongoClient, Collection, Cursor, Database},
};

use actix_web::{
    get,
    http::header::ContentType,
    http::StatusCode,
    web::{Data, Path},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};

// convert to peg
lazy_static! {
    static ref R_INDEX: Result<Regex, Error> = Regex::new(r"index\s*\{");
    static ref R_ERR: Result<Regex, Error> = Regex::new(r"(\b\d{3})\s*\{");
    static ref R_FN: Result<Regex, Error> = Regex::new(r"(\w+)\((.*?)\)\s*\{");
    static ref R_DOT: Result<Regex, Error> = Regex::new(r"\.(\w+)\((.*?)\)\s*\{");
    static ref R_WILD: Result<Regex, Error> = Regex::new(r"\*\s*\{|wildcard\s*\{");
    static ref R_SLASH: Result<Regex, Error> = Regex::new(r"(?m)\/(?=.*\((.*?)\)\s*\{[^{]*$)");
}

fn rm_first(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.as_str()
}

fn convert_to_format(input: &str) -> String {
    let re = Regex::new(r"\.(\w+)").unwrap();
    format!("_route_{}", re.replace_all(&input.replace("/", "_"), |captures: &Captures| format!("__d{}", rm_first(&captures[0]))))
}

fn route_to_fn(input: &str) -> String {
    let re = Regex::new(r#"\{([^{}\s]+)\}"#).unwrap();
    let re_dot = Regex::new(r"\.(\w+)").unwrap();

    let result = re.replace_all(&input, |captures: &regex::Captures| {
        let content = captures.get(1).map_or("", |m| m.as_str());
        format!("_arg_{content}")
    });

    format!(
        "_route_fmt_{}",
        re_dot.replace_all(&result.replace("/", "_"), |captures: &Captures| format!("__d{}", rm_first(&captures[0])))
    )
}

fn convert_status(code: i64) -> StatusCode {
    let u16_code = code as u16;
    StatusCode::from_u16(u16_code).unwrap_or(StatusCode::OK)
}

fn error(engine: &Engine, path: &str, err: ParseError) -> AST {
    match engine.compile(format!("fn {path}(){{text(\"error reading script file: {err}\")}}")) {
        Ok(ast) => ast,
        Err(_) => Default::default(),
    }
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

    (data, content_type, convert_status(status_code))
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
    pub fn text(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::plaintext(), convert_status(status)) }
    pub fn html(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::html(), convert_status(status)) }
    pub fn json(object: Dynamic, status: i64) -> (String, ContentType, StatusCode) {
        match serde_json::to_string(&object) {
            Ok(result) => (result, ContentType::json(), convert_status(status)),
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
            Err(err) => Err(format!("{}", &err).into()),
        }
    }
}

#[export_module]
mod mongo {
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

    impl FromIterator<MongoDynamic> for Vec<Dynamic> {
        fn from_iter<I: IntoIterator<Item = MongoDynamic>>(iter: I) -> Self { iter.into_iter().map(|mongo_dynamic| mongo_dynamic.0).collect() }
    }

    trait IntoDocument {
        fn into(self) -> Document;
    }

    impl IntoDocument for Map {
        fn into(self) -> Document {
            Document::from(
                serde_json::from_str(&match serde_json::to_string(&self) {
                    Ok(data) => data,
                    Err(err) => format!("{{\"err\": \"{err}\"}}"),
                })
                .expect("failed to deserialize"),
            )
        }
    }

    pub fn connect() -> Client {
        let config = config::read().database.unwrap();

        match MongoClient::with_uri_str(config.mongo.unwrap().url.unwrap_or("".to_string())) {
            Ok(client) => Client { client: Some(client) },
            Err(_) => Client { client: None },
        }
    }

    pub fn shutdown(conn: Client) { conn.client.unwrap().shutdown(); }

    #[rhai_fn(global, return_raw, name = "list")]
    pub fn list_databases(conn: Client) -> Result<Dynamic, Box<EvalAltResult>> {
        match conn.client {
            Some(client) => match client.list_databases(None, None) {
                Err(err) => Err(format!("{}", &err).into()),
                Ok(list) => to_dynamic(list),
            },
            None => to_dynamic::<Vec<Dynamic>>(vec![]),
        }
    }

    #[rhai_fn(global, return_raw, name = "count")]
    pub fn count_databases(conn: Client) -> Result<i64, Box<EvalAltResult>> {
        match conn.client {
            Some(client) => match client.list_databases(None, None) {
                Err(err) => Err(format!("{}", &err).into()),
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
    pub fn list_collections(conn: Mongo) -> Result<Dynamic, Box<EvalAltResult>> {
        match conn.db {
            Some(client) => match client.list_collections(None, None) {
                Err(err) => Err(format!("{}", &err).into()),
                Ok(list) => to_dynamic(list.map(|item| item.unwrap()).collect::<Vec<CollectionSpecification>>()),
            },
            None => to_dynamic::<Vec<Dynamic>>(vec![]),
        }
    }

    #[rhai_fn(global, return_raw, name = "get")]
    pub fn collection(conn: Mongo, name: String) -> Result<Collection<MongoDynamic>, Box<EvalAltResult>> {
        match conn.db {
            Some(client) => Ok(client.collection(&name)),
            None => Err("No collection found".into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "count")]
    pub fn count_collections(collection: Collection<MongoDynamic>) -> Result<i64, Box<EvalAltResult>> {
        match collection.count_documents(None, None) {
            Ok(count) => Ok(count as i64),
            Err(err) => Err(format!("{}", &err).into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "find")]
    pub fn find_all(collection: Collection<MongoDynamic>) -> Result<Arc<Cursor<MongoDynamic>>, Box<EvalAltResult>> {
        match collection.find(None, None) {
            Ok(cursor) => Ok(Arc::new(cursor)),
            Err(err) => Err(format!("{}", &err).into()),
        }
    }

    #[rhai_fn(global, return_raw, name = "find")]
    pub fn find_filter(collection: Collection<MongoDynamic>, filter: Map) -> Result<Arc<Cursor<MongoDynamic>>, Box<EvalAltResult>> {
        match collection.find(IntoDocument::into(filter), None) {
            Ok(cursor) => Ok(Arc::new(cursor)),
            Err(err) => Err(format!("{}", &err).into()),
        }
    }

    #[rhai_fn(global, name = "count")]
    pub fn count_cursor(cursor: Arc<Cursor<MongoDynamic>>) -> i64 {
        match Arc::into_inner(cursor) {
            Some(cursor) => cursor.count() as i64,
            None => 0,
        }
    }

    #[rhai_fn(global, return_raw, name = "collect")]
    pub fn collect(cursor: Arc<Cursor<MongoDynamic>>) -> Result<Dynamic, Box<EvalAltResult>> {
        let cursor = Arc::try_unwrap(cursor).expect("Cursor failure");
        match cursor.collect() {
            Ok(items) => to_dynamic::<Vec<Dynamic>>(items),
            Err(err) => to_dynamic::<String>(err.to_string()),
        }
    }
}

#[export_module]
mod kv {
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
            Err(err) => Err(format!("{}", &err).into()),
            Ok(_) => Ok(()),
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
        let response =
            match client.get(url).send() {
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

        let data =
            match serde_json::to_string(&data) {
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
            Err(err) => Err(format!("{}", &err).into()),
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
            Err(err) => Err(format!("{}", &err).into()),
        }
    }
}

#[get("{url:.*}")]
async fn handler(url: Path<String>, req: HttpRequest, config: Data<Config>) -> impl Responder {
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
        _ => convert_to_format(&url.clone()),
    };

    fs_pkg.register_into_engine(&mut engine);
    url_pkg.register_into_engine(&mut engine);

    engine.register_static_module("json", json.into());
    engine.register_static_module("http", http.into());
    engine.register_static_module("exists", exists.into());

    if let Some(database) = &config.database {
        if let Some(_) = &database.kv {
            let kv = exported_module!(kv);
            engine.register_static_module("kv", kv.into());
        }
        if let Some(_) = &database.sqlite {}
        if let Some(_) = &database.mongo {
            let mongo = exported_module!(mongo);
            engine.register_static_module("mongo", mongo.into());
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
                let key =
                    r_path.replace_all(&path, |captures: &regex::Captures| {
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

        let result = R_DOT.as_ref().unwrap().replace_all(&result, |captures: &Captures| format!("__d{}", rm_first(&captures[0]))).to_string();
        let result = R_FN.as_ref().unwrap().replace_all(&result, |captures: &Captures| format!("fn _route_{}", &captures[0])).to_string();

        ternary!(has_wildcard, R_WILD.as_ref().unwrap().replace_all(&result, "fn _wildcard() {").to_string(), result)
    };

    let mut ast = match engine.compile(contents) {
        Ok(ast) => ast,
        Err(err) => error(&engine, &path, err),
    };

    ast.set_source(filename.to_string_lossy().to_string());

    if url.clone() == "" && has_index {
        let (body, content_type, status_code) = engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, "_route_index", ()).unwrap();
        tracing::info!(
            method = string!(req.method()),
            status = string!(status_code),
            content = string!(content_type),
            "request '{}'",
            req.uri()
        );
        return HttpResponse::build(status_code).content_type(content_type).body(body);
    };

    for (route, args) in routes {
        let url = url.clone();
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        if url == route {
            match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, convert_to_format(&url.clone()), ()) {
                Ok(response) => {
                    let (body, content_type, status_code) = response;
                    tracing::info!(
                        method = string!(req.method()),
                        status = string!(status_code),
                        content = string!(content_type),
                        "request '{}'",
                        req.uri()
                    );
                    return HttpResponse::build(status_code).content_type(content_type).body(body);
                }
                Err(err) => {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body(format!("Internal Server Error\n\n{err}"));
                }
            }
        }

        match match_route(&route, &args, &url) {
            Some(data) => match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, route_to_fn(&route), data) {
                Ok(response) => {
                    let (body, content_type, status_code) = response;
                    tracing::info!(
                        method = string!(req.method()),
                        status = string!(status_code),
                        content = string!(content_type),
                        "request '{}'",
                        req.uri()
                    );
                    return HttpResponse::build(status_code).content_type(content_type).body(body);
                }
                Err(err) => {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body(format!("Internal Server Error\n\n{err}"));
                }
            },
            None => {}
        }
    }

    let (body, content_type, status_code) = {
        if has_wildcard || has_error_page {
            engine
                .call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, ternary!(has_wildcard, "_wildcard", "_route_error_404"), ())
                .unwrap()
        } else {
            eprintln!("Error reading script file: {}", filename.to_string_lossy());
            (
                format!("function not found.\ndid you create {url}()?\n\nyou can add * {{}} or 404 {{}} routes as well."),
                ContentType::plaintext(),
                StatusCode::NOT_FOUND,
            )
        }
    };

    let status_code = ternary!(has_wildcard, status_code, StatusCode::NOT_FOUND);
    tracing::info!(
        method = string!(req.method()),
        status = string!(status_code),
        content = string!(content_type),
        "request '{}'",
        req.uri()
    );
    return HttpResponse::build(status_code).content_type(content_type).body(body);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env::set_var("RUST_LOG", "INFO");

    let config = config::read();
    let app = || App::new().app_data(Data::new(config::read())).service(handler);

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
