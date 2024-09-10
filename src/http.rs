use crate::{
    helpers,
    helpers::prelude::*,
    modules::prelude::*,
    routes::prelude::*,
    structs::{config::*, template::*},
};

use mime::Mime;
use reqwest::blocking::Client as ReqwestClient;
use rhai_dynamic::ToDynamic;
use std::{collections::HashMap, io, sync::Arc};

use rhai::{exported_module as export, plugin::*, Dynamic, Engine, Map, Scope};

use actix_web::{
    http::{header::ContentType, StatusCode, Uri},
    web::{self, Data},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};

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

fn parse_bool(s: &str) -> bool {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        _ => false,
    }
}

fn parse_slash(s: &str) -> String {
    let parts: Vec<&str> = s.splitn(3, '/').collect();
    if parts.len() > 1 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        s.to_string()
    }
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

struct Handler<'h> {
    url: &'h Uri,
    path: &'h str,
    engine: &'h mut Engine,
    scope: &'h mut Scope<'h>,
}

impl<'h> Handler<'h> {
    fn url(&self) -> String { self.url.to_string() }
}

async fn handler(req: HttpRequest, config: Data<Arc<Config>>) -> Result<impl Responder, actix_web::Error> {
    let mut modules = Modules::new();

    let app = Handler {
        url: req.uri(),
        path: req.path(),
        engine: &mut Engine::new(),
        scope: &mut Scope::new(),
    };

    modules.builtin(app.engine);
    modules.register("json", export!(json));
    modules.register("http", export!(http));
    modules.register("exists", export!(exists));

    modules.get_ext().for_each(|ext| {
        app.engine.register_static_module(ext.0, ext.1);
    });

    if let Some(database) = &config.database {
        if let Some(_) = &database.kv {
            let kv = exported_module!(kv_db);
            app.engine.register_static_module("kv", kv.into());
        }
        if let Some(_) = &database.mongo {
            let mongo = exported_module!(mongo_db);
            app.engine.register_static_module("mongo", mongo.into());
        }
        if let Some(_) = &database.redis {
            let redis = exported_module!(redis_db);
            app.engine.register_static_module("redis", redis.into());
        }
    }

    #[derive(Clone, ToDynamic)]
    struct Request {
        path: String,
        url: String,
        version: String,
        query: String,
    }

    #[derive(Clone, ToDynamic)]
    struct Internal {
        version: String,
    }

    let request = Request {
        url: app.url.to_string(),
        path: app.path.to_owned(),
        version: format!("{:?}", req.version()),
        query: req.query_string().to_string(),
    };

    let internal = Internal {
        version: format!("{:?}", req.version()),
    };

    app.scope.push("app", internal.to_dynamic());
    app.scope.push("request", request.to_dynamic());

    app.engine
        .register_fn("cwd", cwd)
        .register_fn("proxy", proxy)
        .register_fn("response", response)
        .register_fn("text", default::text)
        .register_fn("json", default::json)
        .register_fn("html", default::html)
        .register_fn("text", status::text)
        .register_fn("json", status::json)
        .register_fn("html", status::html);

    let contents = get_workers(&config.workers).await?;

    if let Err(err) = parse::try_parse(&contents).await {
        error!(req->err@app.url);
    };

    let (route, args) = match Route::get(parse_slash(&app.url())).await {
        Ok(route) => {
            let mut matched_url = app.url();

            let cfg = match route.cfg {
                Some(cfg) => cfg,
                None => HashMap::new(),
            };

            for (item, val) in cfg {
                // convert to enum Cfg::Wildcard, etc
                match item.as_str() {
                    "wildcard" => {
                        if parse_slash(&app.url()) == route.route && parse_bool(&val) {
                            matched_url = parse_slash(&app.url());
                            break;
                        }
                    }
                    _ => {}
                }
            }

            match Route::get(matched_url.to_owned()).await {
                Ok(matched) => (matched, vec![]),
                Err(err) => match Route::search_for(matched_url).await {
                    Some(matched) => matched,
                    None => error!(req->err@app.url),
                },
            }
        }
        Err(err) => match Route::search_for(app.url()).await {
            Some(matched) => matched,
            None => error!(req->err@app.url),
        },
    };

    let mut ast = match app.engine.compile(route.construct_fn()) {
        Ok(ast) => ast,
        // fix fn name error
        Err(err) => helpers::error(&app.engine, &app.url(), err),
    };

    ast.set_source("runtime::workers");

    let fn_name = match route.fn_name.as_str() {
        "/" => "/index",
        name => name,
    };

    match app.engine.call_fn::<(String, ContentType, StatusCode)>(app.scope, &ast, fn_name, args) {
        Ok(response) => send!(req->response),
        Err(err) => {
            let body = ServerError {
                error: err.to_string().replace("\n", "<br>"),
                context: vec![],
            };

            return Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).content_type(ContentType::html()).body(body.render().unwrap()));
        }
    };
}

#[tokio::main]
pub async fn start(config: Config) -> io::Result<()> {
    let owned = Arc::new(config.to_owned());

    let app = move || {
        let config = Arc::clone(&owned);
        App::new().app_data(Data::new(config)).default_service(web::to(handler))
    };

    log::info!(address = config.settings.address, port = config.settings.port, "server started");
    Ok(HttpServer::new(app).bind(config.get_address())?.run().await?)
}
