pub mod file;

use actix_web::http::StatusCode;
use mongodb::{bson::doc, sync::Database};
use regex::{Captures, Regex};
use rhai::{plugin::EvalAltResult, Engine, ParseError, AST};

pub mod prelude {
    pub use super::file::*;
}

pub fn rm_first(s: &str) -> &str {
    let mut chars = s.chars();
    chars.next();
    chars.as_str()
}

pub fn convert_to_format(input: &str) -> String {
    let re = Regex::new(r"\.(\w+)").unwrap();
    let input = super::replace_chars(input);
    format!("_route_{}", re.replace_all(&input.replace("/", "_"), |captures: &Captures| format!("__d{}", rm_first(&captures[0]))))
}

pub fn route_to_fn(input: &str) -> String {
    let input = super::replace_chars(input);
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

pub fn collection_exists(d: &Database, name: &String) -> Result<bool, Box<EvalAltResult>> {
    let filter = doc! { "name": &name };

    match d.list_collection_names(Some(filter)) {
        Err(err) => Err(err.to_string().into()),
        Ok(list) => Ok(list.into_iter().any(|col| col == *name)),
    }
}

pub fn convert_status(code: i64) -> StatusCode {
    let u16_code = code as u16;
    StatusCode::from_u16(u16_code).unwrap_or(StatusCode::OK)
}

pub fn error(engine: &Engine, path: &str, err: ParseError) -> AST {
    match engine.compile(format!("fn {path}(){{text(\"error reading script file: {err}\")}}")) {
        Ok(ast) => ast,
        Err(_) => Default::default(),
    }
}
