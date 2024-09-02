mod config;
mod database;
mod globals;
mod helpers;
mod modules;
mod routes;
mod structs;

use helpers::prelude::*;
use modules::prelude::*;
use structs::{config::*, template::*};

use mime::Mime;

use regex::{Captures, Error, Regex};
use reqwest::blocking::Client as ReqwestClient;
use smartstring::alias::String as SmString;
use std::{collections::HashMap, fs};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{filter::LevelFilter, prelude::*};

use rhai::{packages::Package, plugin::*, Dynamic, Engine, Map, Scope};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;

use macros_rs::{
    exp::ternary,
    fmt::{crashln, string},
    obj::lazy_lock,
    os::set_env_sync,
};

use actix_web::{
    http::{header::ContentType, StatusCode},
    web::{self, Data},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};

lazy_lock! {
    static R_INDEX: Result<Regex, Error> = Regex::new(r"index\s*\{");
    static R_ERR: Result<Regex, Error> = Regex::new(r"(\b\d{3})\s*\{");
    static R_DOT: Result<Regex, Error> = Regex::new(r"\.(\w+)\((.*?)\)\s*\{");
    static R_WILD: Result<Regex, Error> = Regex::new(r"\*\s*\{|wildcard\s*\{");
    static R_FN: Result<Regex, Error> = Regex::new(r"([\w#:\-@!&^~]+)\((.*?)\)\s*\{");
    static R_SLASH: Result<Regex, Error> = Regex::new(r"(?m)\/(?=.*\((.*?)\)\s*\{[^{]*$)");
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

fn parse_bool(s: &str) -> bool {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        _ => false,
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

pub fn replace_chars(input: &str) -> String {
    let replacements = HashMap::from([
        ('#', "__fhas"),
        (':', "__fcol"),
        ('-', "__fdas"),
        ('@', "__fats"),
        ('!', "__fexl"),
        ('&', "__famp"),
        ('^', "__fcar"),
        ('~', "__ftil"),
    ]);

    let mut result = String::with_capacity(input.len());

    for c in input.chars() {
        if let Some(replacement) = replacements.get(&c) {
            result.push_str(replacement);
        } else {
            result.push(c);
        }
    }

    result
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
        // remove this
        return HttpResponse::Ok().body("");
    }

    let mut routes: HashMap<String, Vec<String>> = HashMap::new();

    let filename = &config.workers.get(0).unwrap();
    let fs_pkg = FilesystemPackage::new();
    let url_pkg = UrlPackage::new();

    let json = exported_module!(json);
    let http = exported_module!(http);
    let exists = exported_module!(exists);

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
        .register_fn("cwd", cwd)
        .register_fn("proxy", proxy)
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

    routes::parse::try_parse(&contents);

    let has_error_page = R_ERR.as_ref().unwrap().is_match(&contents).unwrap();
    let has_wildcard = R_WILD.as_ref().unwrap().is_match(&contents).unwrap();
    let has_index = R_INDEX.as_ref().unwrap().is_match(&contents).unwrap();

    fn parse_cfg(cfg_str: &str) -> HashMap<String, String> {
        cfg_str
            .split(',')
            .filter_map(|pair| {
                let mut parts = pair.split('=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    Some((key.trim().to_string(), value.trim().trim_matches('"').to_string()))
                } else {
                    None
                }
            })
            .collect()
    }

    let contents = {
        let pattern = r#"\{([^{}\s]+)\}"#;
        let pattern_rm_config = r#",?\s*cfg\([^)]*\)"#;
        let pattern_combine = r#"(?m)^_route/(.*)\n(.*?)\((.*?)\)"#;

        let re = Regex::new(pattern).unwrap();
        let re_combine = Regex::new(pattern_combine).unwrap();
        let re_rm_config = Regex::new(pattern_rm_config).unwrap();

        let result = re.replace_all(&contents, |captures: &regex::Captures| {
            let content = captures.get(1).map_or("", |m| m.as_str());
            format!("_arg_{content}")
        });

        let result = re_rm_config.replace_all(&result, "");
        let result = result.replace("#[route(\"", "_route").replace("\")]", "");

        let new_result_fmt = re_combine
            .replace_all(&result, |captures: &regex::Captures| {
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
            .into_owned();

        std::mem::drop(result);

        new_result_fmt
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

        let result = R_FN
            .as_ref()
            .unwrap()
            .replace_all(&result, |captures: &Captures| {
                let fmt = replace_chars(&captures[0]);
                format!("fn _route_{fmt}")
            })
            .to_string();

        ternary!(has_wildcard, R_WILD.as_ref().unwrap().replace_all(&result, "fn _wildcard() {").to_string(), result)
    };

    let contents = {
        let slash = Regex::new(r"\$\((.*?)\)").unwrap();
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

    //     for data in route_data {
    //         let name = data.route;
    //
    //         let cfg = match data.cfg {
    //             Some(cfg) => cfg,
    //             None => HashMap::new(),
    //         };
    //
    //         for (item, val) in cfg {
    //             match item.as_str() {
    //                 "wildcard" => {
    //                     if url.splitn(2, '/').next().unwrap_or(&url) == name && parse_bool(&val) {
    //                         match engine.call_fn::<(String, ContentType, StatusCode)>(&mut scope, &ast, helpers::convert_to_format(&name), ()) {
    //                             Ok(response) => send!(response),
    //                             Err(err) => {
    //                                 let body = ServerError {
    //                                     error: err.to_string().replace("\n", "<br>"),
    //                                     context: extract_context(contents, err.to_string()),
    //                                 };
    //
    //                                 return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).content_type(ContentType::html()).body(body.render().unwrap());
    //                             }
    //                         }
    //                     }
    //                 }
    //                 _ => {}
    //             }
    //         }
    //     }

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
    set_env_sync!(RUST_LOG = "info");
    globals::init();

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
