pub mod parse;

use chrono::{DateTime, Duration, Utc};
use global_placeholders::global;
use macros_rs::fmt::string;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use smartstring::{LazyCompact, SmartString};

use std::{
    collections::HashMap,
    fs::{create_dir_all, read, write},
    path::{Path, PathBuf},
};

pub type RtData = SmartString<LazyCompact>;
pub type RtArgs = Option<Vec<RtData>>;
pub type RtConfig = Option<HashMap<String, String>>;
pub type RtTime = DateTime<Utc>;

pub enum RtKind {
    Index,
    Wildcard,
    NotFound,
    Normal,
}

#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Route {
    pub cfg: RtConfig,
    pub args: RtArgs,
    pub hash: String,
    pub expires: RtTime,
    pub created: RtTime,
    pub cache: PathBuf,
    pub route: RtData,
    pub fn_name: RtData,
    pub fn_body: RtData,
    pub start_pos: usize,
    pub end_pos: usize,
}

impl Route {
    pub fn default() -> Self { Default::default() }

    pub fn cache(&mut self, kind: RtKind) -> &Self {
        let mut md5 = Md5::new();

        let route_name = match kind {
            RtKind::Index => "/index",
            RtKind::Wildcard => "/wildcard",
            RtKind::NotFound => "/not_found",
            RtKind::Normal => self.route.as_str(),
        };

        let fn_name = match kind {
            RtKind::Index => "index",
            RtKind::Wildcard => "wildcard",
            RtKind::NotFound => "not_found",
            RtKind::Normal => self.fn_name.as_str(),
        };

        let now = Utc::now();
        let cache_key = global!("dirs.cache", route_name);

        self.route = route_name.into();
        self.fn_name = fn_name.into();

        md5.update(&self.route);
        md5.update(&self.fn_name);
        md5.update(&self.fn_body);

        self.created = now;
        self.expires = now + Duration::hours(3);
        self.cache = Path::new(&cache_key).to_owned();
        self.hash = const_hex::encode(md5.finalize());

        return self;
    }

    pub fn save(&mut self, kind: RtKind) {
        self.cache(kind);

        if let Some(parent) = self.cache.parent() {
            if !parent.exists() {
                // add error handling
                create_dir_all(parent).unwrap();
            }
        }

        let encoded = match ron::ser::to_string(&self) {
            Ok(contents) => contents,
            Err(err) => {
                tracing::error!(err = string!(err), "Cannot encode route");
                std::process::exit(1);
            }
        };

        if let Err(err) = write(self.cache.to_owned(), encoded) {
            tracing::error!(err = string!(err), "Error writing route");
            std::process::exit(1);
        }
    }

    pub fn get(key: &str) -> String {
        let bytes = match read(&key) {
            Ok(contents) => contents,
            Err(err) => {
                tracing::error!(err = string!(err), "Error reading route");
                std::process::exit(1);
            }
        };

        let data: Route = match ron::de::from_bytes(&bytes) {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::error!(err = string!(err), "Error reading route");
                std::process::exit(1);
            }
        };

        let args = match data.args {
            Some(args) => match args.len() {
                0 => String::new(),
                1 => args[0].to_string(),
                _ => args.join(", "),
            },
            None => "".into(),
        };

        format!("fn {}({args}){{{}", data.fn_name, data.fn_body)
    }
}

pub mod prelude {
    pub use super::Route;
    pub use super::RtConfig;
    pub use super::RtData;
    pub use super::RtKind;
}
