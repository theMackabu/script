use fancy_regex::{Captures, Error, Regex};
use lazy_static::lazy_static;
use macros_rs::{crashln, ternary};
use reqwest::blocking::Client;
use std::{fs, path::PathBuf};

use rhai::{packages::Package, Engine, Map, ParseError, Scope, AST};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use actix_web::{get, http::header::ContentType, web::Path, App, HttpRequest, HttpResponse, HttpServer, Responder};

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

fn text(string: String) -> (String, ContentType) { (string, ContentType::plaintext()) }
fn html(string: String) -> (String, ContentType) { (string, ContentType::html()) }

fn get(url: String) -> String {
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

fn json(object: Map) -> (String, ContentType) {
    match serde_json::to_string(&object) {
        Ok(result) => (result, ContentType::json()),
        Err(err) => (err.to_string(), ContentType::plaintext()),
    }
}

fn error(engine: &Engine, path: &str, err: ParseError) -> AST {
    match engine.compile(format!("fn {path}(){{text(\"error reading script file: {err}\")}}")) {
        Ok(ast) => ast,
        Err(_) => Default::default(),
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

    let mut engine = Engine::new();
    let mut scope = Scope::new();

    let path = match url.as_str() {
        "" => "_index".to_string(),
        _ => convert_to_format(&url.clone()),
    };

    fs_pkg.register_into_engine(&mut engine);
    url_pkg.register_into_engine(&mut engine);

    scope
        .push_constant("path", url.to_string())
        .push_constant("url", req.uri().to_string())
        .push_constant("ver", format!("{:?}", req.version()))
        .push_constant("query", req.query_string().to_string());

    engine.register_fn("get", get).register_fn("text", text).register_fn("json", json).register_fn("html", html);

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

    let (body, content_type) = match engine.call_fn::<(String, ContentType)>(&mut scope, &ast, &path, ()) {
        Ok(response) => response,
        Err(err) => {
            if has_wildcard || has_error_page {
                engine
                    .call_fn::<(String, ContentType)>(&mut scope, &ast, ternary!(has_wildcard, "_wildcard", "_error_404"), (err.to_string(),))
                    .unwrap()
            } else {
                eprintln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err);
                (
                    format!("function not found.\ndid you create {url}()?\n\nyou can add * {{}} or 404 {{}} routes as well."),
                    ContentType::plaintext(),
                )
            }
        }
    };

    HttpResponse::Ok().content_type(content_type).body(body)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || App::new().service(handler);
    let addr = ("127.0.0.1", 3000);

    println!("listening on {:?}", addr);
    HttpServer::new(app).bind(addr).unwrap().run().await
}
