use crate::helpers::convert_status;
use actix_web::http::{header::ContentType, StatusCode};
use rhai::plugin::*;

#[export_module]
pub mod default {
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
pub mod status {
    pub fn text(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::plaintext(), convert_status(status)) }

    pub fn html(string: String, status: i64) -> (String, ContentType, StatusCode) { (string, ContentType::html(), convert_status(status)) }

    pub fn json(object: Dynamic, status: i64) -> (String, ContentType, StatusCode) {
        match serde_json::to_string(&object) {
            Ok(result) => (result, ContentType::json(), convert_status(status)),
            Err(err) => (err.to_string(), ContentType::plaintext(), StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
