use fancy_regex::{Captures, Error, Regex};
use lazy_static::lazy_static;
use macros_rs::{crashln, ternary};
use reqwest::blocking::Client;
use std::{fs, path::PathBuf};

use rhai::{packages::Package, plugin::*, Engine, FnNamespace, Map, ParseError, Scope, AST};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use actix_web::{get, http::header::ContentType, http::StatusCode, web::Path, App, HttpRequest, HttpResponse, HttpServer, Responder};

// convert to peg
lazy_static! {
    static ref R_INDEX: Result<Regex, Error> = Regex::new(r"index\s*\{");
    static ref R_ERR: Result<Regex, Error> = Regex::new(r"(\b\d{3})\s*\{");
    static ref R_DOT: Result<Regex, Error> = Regex::new(r"\.(\w+)\(\)\s*\{");
    static ref R_FN: Result<Regex, Error> = Regex::new(r"(\w+)\((.*?)\)\s*\{");
    static ref R_WILD: Result<Regex, Error> = Regex::new(r"\*\s*\{|wildcard\s*\{");
    static ref R_SLASH: Result<Regex, Error> = Regex::new(r"(?<=[a-zA-Z])/(?=[a-zA-Z])");
}

fn rm_first(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.as_str()
}

fn convert_to_format(input: &str) -> String {
    Regex::new(r"\.(\w+)")
        .unwrap()
        .replace_all(&input.replace("/", "_"), |captures: &Captures| format!("__d{}", rm_first(&captures[0])))
        .to_string()
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

#[export_module]
mod default {
    pub fn text(string: String) -> (String, ContentType, StatusCode) { (string, ContentType::plaintext(), StatusCode::OK) }
    pub fn html(string: String) -> (String, ContentType, StatusCode) { (string, ContentType::html(), StatusCode::OK) }
    pub fn json(object: Map) -> (String, ContentType, StatusCode) {
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
    pub fn json(object: Map, status: i64) -> (String, ContentType, StatusCode) {
        match serde_json::to_string(&object) {
            Ok(result) => (result, ContentType::json(), convert_status(status)),
            Err(err) => (err.to_string(), ContentType::plaintext(), StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[export_module]
mod http {
    pub fn get(url: String) -> String {
        let client = Client::new();
        let response = match client.get(url).send() {
            Ok(res) => res,
            Err(err) => return err.to_string(),
        };

        if response.status().is_success() {
            response.text().unwrap()
        } else {
            format!("request failed with status code: {}", response.status())
        }
    }
}

#[get("{url:.*}")]
async fn handler(url: Path<String>, req: HttpRequest) -> impl Responder {
    if url.as_str() == "favicon.ico" {
        return HttpResponse::Ok().body("");
    }

    let filename: PathBuf = "app.routes".into();
    let fs_pkg = FilesystemPackage::new();
    let url_pkg = UrlPackage::new();
    let http = exported_module!(http);

    let mut engine = Engine::new();
    let mut scope = Scope::new();

    let path = match url.as_str() {
        "" => "_index".to_string(),
        _ => convert_to_format(&url.clone()),
    };

    fs_pkg.register_into_engine(&mut engine);
    url_pkg.register_into_engine(&mut engine);
    engine.register_static_module("http", http.into());

    scope
        .push_constant("path", url.to_string())
        .push_constant("url", req.uri().to_string())
        .push_constant("ver", format!("{:?}", req.version()))
        .push_constant("query", req.query_string().to_string());

    engine
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

    // cache contents until file change
    let contents = {
        let result = R_SLASH.as_ref().unwrap().replace_all(&contents, "_").to_string();
        let result = R_INDEX.as_ref().unwrap().replace_all(&result, "_index() {").to_string();
        let result = R_ERR.as_ref().unwrap().replace_all(&result, "_error_$1(err) {").to_string();
        let result = R_DOT.as_ref().unwrap().replace_all(&result, |captures: &Captures| format!("__d{}", rm_first(&captures[0]))).to_string();
        let result = R_FN.as_ref().unwrap().replace_all(&result, |captures: &Captures| format!("fn {}", &captures[0])).to_string();

        ternary!(has_wildcard, R_WILD.as_ref().unwrap().replace_all(&result, "fn _wildcard(err) {").to_string(), result)
    };

    println!("{contents}");

    let mut ast = match engine.compile(contents) {
        Ok(ast) => ast,
        Err(err) => error(&engine, &path, err),
    };

    ast.set_source(filename.to_string_lossy().to_string());

    let (body, content_type, status_code) = match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, &path, ()) {
        Ok(response) => response,
        Err(err) => {
            if has_wildcard || has_error_page {
                engine
                    .call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, ternary!(has_wildcard, "_wildcard", "_error_404"), (err.to_string(),))
                    .unwrap()
            } else {
                eprintln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err);
                (
                    format!("function not found.\ndid you create {url}()?\n\nyou can add * {{}} or 404 {{}} routes as well."),
                    ContentType::plaintext(),
                    StatusCode::NOT_FOUND,
                )
            }
        }
    };

    HttpResponse::build(status_code).content_type(content_type).body(body)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || App::new().service(handler);
    let addr = ("127.0.0.1", 3000);

    println!("listening on {:?}", addr);
    HttpServer::new(app).bind(addr).unwrap().run().await
}
