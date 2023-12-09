use macros_rs::{crashln, str, ternary};
use once_cell::sync::Lazy;
use regex::Regex;
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

use rhai::{packages::Package, Engine, Map, ParseError, Scope, AST};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use actix_web::{get, http::header::ContentType, web::Path, App, HttpRequest, HttpResponse, HttpServer, Responder};

static GET_ROUTES: Lazy<Mutex<BTreeMap<String, String>>> = Lazy::new(|| Mutex::new(BTreeMap::new()));

fn remove_first(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.as_str()
}

fn convert_to_format(input: &str) -> String {
    Regex::new(r"\.(\w+)")
        .unwrap()
        .replace_all(&input.replace("/", "_"), |captures: &regex::Captures| format!("__d{}", remove_first(&captures[0])))
        .to_string()
}

fn get(path: String, fn_name: String) {
    let mut routes = GET_ROUTES.lock().unwrap();
    routes.insert(path, fn_name);
}

fn text(string: String) -> (String, ContentType) { (string, ContentType::plaintext()) }
fn html(string: String) -> (String, ContentType) { (string, ContentType::html()) }

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
        "" => "index".to_string(),
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

    let has_error_page = Regex::new(r"\b\d{3}\s*\{").unwrap().is_match(&contents);
    let has_wildcard = Regex::new(r"\*\s*\{|wildcard\s*\{").unwrap().is_match(&contents);

    let contents = {
        let p_dot = Regex::new(r"\.(\w+)\(\)\s*\{").unwrap();
        let p_fn = Regex::new(r"(\w+)\(\)\s*\{").unwrap();
        let p_wild = Regex::new(r"\*\s*\{|wildcard\s*\{").unwrap();

        let result = contents.replace("/", "_");
        let result = p_dot.replace_all(&result, |captures: &regex::Captures| format!("__d{}", remove_first(&captures[0]))).to_string();
        let result = p_fn.replace_all(str!(result), |captures: &regex::Captures| format!("fn {}", &captures[0])).to_string();

        ternary!(has_wildcard, p_wild.replace_all(&result, "fn wildcard() {").to_string(), result)
    };

    let mut ast = match engine.compile(contents) {
        Ok(ast) => ast,
        Err(err) => error(&engine, &path, err),
    };

    ast.set_source(filename.to_string_lossy().to_string());

    let (body, content_type) = match engine.call_fn::<(String, ContentType)>(&mut scope, &ast, &path, ()) {
        Ok(response) => response,
        Err(err) => {
            if has_wildcard {
                engine.call_fn::<(String, ContentType)>(&mut scope, &ast, "wildcard", ()).unwrap()
            } else {
                eprintln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err);
                (format!("function not found.\ndid you create {url}()?"), ContentType::plaintext())
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
