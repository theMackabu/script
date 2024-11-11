use crate::structs::modules::*;
use macros_rs::fmt::{str, string};
use reqwest::blocking::Client as ReqwestClient;
use rhai::{plugin::*, FnNamespace, Map};
use smartstring::alias::String as SmString;

#[export_module]
pub mod http {
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
        let response = match client.get(url).send() {
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
                body: Some(response.text().unwrap_or_default()),
            }
        } else {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: Some(response.text().unwrap_or_default()),
                body: None,
            }
        }
    }

    pub fn post(url: String, data: Map) -> Http {
        let client = ReqwestClient::new();

        let data = match serde_json::to_string(&data) {
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
                body: Some(response.text().unwrap_or_default()),
            }
        } else {
            Http {
                length: response.content_length(),
                status: response.status().as_u16(),
                err: Some(response.text().unwrap_or_default()),
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
        let body = str!(res.body.clone().unwrap_or_default());
        match serde_json::from_str(body) {
            Ok(map) => Ok(map),
            Err(err) => Err(err.to_string().into()),
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
            Err(err) => Err(err.to_string().into()),
        }
    }
}
