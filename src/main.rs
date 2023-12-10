use fancy_regex::{Captures, Error, Regex};
use lazy_static::lazy_static;
use macros_rs::{crashln, str, string, ternary};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use smartstring::alias::String as SmString;
use std::{collections::BTreeMap, fs, path::PathBuf};

use rhai::{packages::Package, plugin::*, Dynamic, Engine, FnNamespace, Map, ParseError, Scope, AST};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use actix_web::{get, http::header::ContentType, http::StatusCode, web::Path, App, HttpRequest, HttpResponse, HttpServer, Responder};

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
    let result = re.replace_all(&input, |captures: &fancy_regex::Captures| {
        let content = captures.get(1).map_or("", |m| m.as_str());
        format!("_arg_{content}")
    });

    format!("_route_fmt_{}", &result.replace("/", "_"))
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

fn match_route(route_template: &str, placeholders: &[&str], url: &str) -> bool {
    let route_segments: Vec<&str> = route_template.split('/').collect();
    let url_segments: Vec<&str> = url.split('/').collect();

    if route_segments.len() != url_segments.len() {
        return false;
    }

    for (route_segment, url_segment) in route_segments.iter().zip(url_segments.iter()) {
        if !match_segment(route_segment, url_segment, placeholders) {
            return false;
        }
    }

    true
}

fn match_segment(route_segment: &str, url_segment: &str, placeholders: &[&str]) -> bool {
    if route_segment.starts_with('{') && route_segment.ends_with('}') {
        let placeholder = &route_segment[1..route_segment.len() - 1];
        placeholders.contains(&placeholder)
    } else {
        route_segment == url_segment
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
    #[derive(Debug, Clone, Serialize, Deserialize)]
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
        let client = Client::new();
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
        let client = Client::new();

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
async fn handler(url: Path<String>, req: HttpRequest) -> impl Responder {
    if url.as_str() == "favicon.ico" {
        return HttpResponse::Ok().body("");
    }

    let mut routes: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let filename: PathBuf = "app.routes".into();
    let fs_pkg = FilesystemPackage::new();
    let url_pkg = UrlPackage::new();
    let http = exported_module!(http);

    let mut engine = Engine::new();
    let mut scope = Scope::new();

    let path = match url.as_str() {
        "" => "_route_index".to_string(),
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
    let has_index = R_INDEX.as_ref().unwrap().is_match(&contents).unwrap();

    let contents = {
        let pattern = r#"\{([^{}\s]+)\}"#;
        let pattern_combine = r#"(?m)^_route/(.*)\n(.*?)\((.*?)\)"#;

        let re = Regex::new(pattern).unwrap();
        let re_combine = Regex::new(pattern_combine).unwrap();

        let result =
            re.replace_all(&contents, |captures: &fancy_regex::Captures| {
                let content = captures.get(1).map_or("", |m| m.as_str());
                format!("_arg_{content}")
            });

        let output = result.replace("#[route(\"", "_route").replace("\")]", "");

        re_combine.replace_all(str!(output), |captures: &fancy_regex::Captures| {
            let path = captures.get(1).map_or("", |m| m.as_str());
            let args = captures.get(3).map_or("", |m| m.as_str());

            if args != "" {
                let r_path = Regex::new(r"(?m)_arg_(\w+)").unwrap();
                let key = r_path.replace_all(&path, |captures: &fancy_regex::Captures| {
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
        println!("{}: {} (status={}, type={})", req.method(), req.uri(), status_code, content_type);
        return HttpResponse::build(status_code).content_type(content_type).body(body);
    };

    for (route, args) in routes {
        let url = url.clone();
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        if url == route {
            match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, convert_to_format(&url.clone()), ()) {
                Ok(response) => {
                    let (body, content_type, status_code) = response;
                    println!("{}: {} (status={}, type={})", req.method(), req.uri(), status_code, content_type);
                    return HttpResponse::build(status_code).content_type(content_type).body(body);
                }
                Err(err) => {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body(format!("Internal Server Error\n\n{err}"));
                }
            }
        }

        if match_route(&route, &args, &url) {
            match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, route_to_fn(&route), ()) {
                Ok(response) => {
                    let (body, content_type, status_code) = response;
                    println!("{}: {} (status={}, type={})", req.method(), req.uri(), status_code, content_type);
                    return HttpResponse::build(status_code).content_type(content_type).body(body);
                }
                Err(err) => {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body(format!("Internal Server Error\n\n{err}"));
                }
            }
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

    return HttpResponse::build(status_code).content_type(content_type).body(body);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || App::new().service(handler);
    let addr = ("127.0.0.1", 3000);

    println!("listening on {:?}", addr);
    HttpServer::new(app).bind(addr).unwrap().run().await
}
