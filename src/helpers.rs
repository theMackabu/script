pub mod colors;
pub mod file;
pub mod macros;

use actix_web::http::StatusCode;
use mongodb::{bson::doc, sync::Database};
use rhai::{plugin::EvalAltResult, Engine, ParseError, AST};

pub mod prelude {
    pub use super::colors::*;
    pub use super::file::*;
    pub use crate::{error, send};
    pub use pat::Tap;
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
