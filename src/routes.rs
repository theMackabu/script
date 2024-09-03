pub mod parse;

use anyhow::{anyhow, Error};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use global_placeholders::global;
use macros_rs::{fmt::string, fs::file_exists, obj::lazy_lock};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use smartstring::{LazyCompact, SmartString};
use tokio::sync::Mutex;
use walkdir::WalkDir;

use tokio::fs::{read, write};

use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, read_dir, remove_dir, remove_file},
    mem::take,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type RtTime = DateTime<Utc>;
pub type RtIndex = (String, Route);
pub type RtData = SmartString<LazyCompact>;
pub type RtArgs = Option<Vec<RtData>>;
pub type RtConfig = Option<HashMap<String, String>>;
pub type RtGlobalIndex = Arc<Mutex<DashMap<String, RouteContainer>>>;

pub enum RtKind {
    Normal,
    Wildcard,
    NotFound,
}

pub struct RouteContainer {
    inner: Route,
    present_in_current_update: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
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

lazy_lock! {
    pub static ROUTES_INDEX: RtGlobalIndex = Arc::new(Mutex::new(DashMap::new()));
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

impl Route {
    pub fn default() -> Self { Default::default() }

    pub async fn cleanup() -> std::io::Result<()> {
        let cache_dir = PathBuf::from(global!("base.cache"));
        let routes = ROUTES_INDEX.lock().await;

        tracing::trace!("Cache directory: {:?}", cache_dir);

        let valid_cache_files: HashSet<PathBuf> = routes
            .iter()
            .map(|item| {
                let path = item.value().inner.cache.clone();
                tracing::trace!("Valid cache file: {:?}", path);
                path
            })
            .collect();

        for entry in WalkDir::new(&cache_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if path.is_file() {
                tracing::trace!("Checking file: {:?}", path);

                let should_keep = valid_cache_files.iter().any(|valid_path| {
                    let paths_match = path == *valid_path;
                    tracing::trace!("Comparing {:?} with {:?}: {}", path, valid_path, paths_match);
                    paths_match
                });

                if !should_keep {
                    tracing::debug!("Deleting file: {:?}", path);
                    remove_file(&path)?;
                } else {
                    tracing::debug!("Keeping file: {:?}", path);
                }
            } else if entry.file_type().is_dir() {
                if read_dir(path.to_owned())?.next().is_none() {
                    tracing::debug!("Removing empty directory: {:?}", path);
                    remove_dir(&path)?;
                }
            }
        }

        Ok(())
    }

    pub async fn update_index(new_routes: Vec<RtIndex>) {
        let routes = ROUTES_INDEX.lock().await;

        for mut entry in routes.iter_mut() {
            entry.present_in_current_update = false;
        }

        for (key, value) in new_routes {
            routes
                .entry(key)
                .and_modify(|e| {
                    e.inner = value.to_owned();
                    e.present_in_current_update = true;
                })
                .or_insert(RouteContainer {
                    inner: value,
                    present_in_current_update: true,
                });
        }

        routes.retain(|_, v| v.present_in_current_update);
    }

    pub fn cache(&mut self, kind: RtKind) -> &Self {
        let now = Utc::now();
        let mut md5 = Md5::new();

        let route_name = match kind {
            RtKind::Wildcard => "/wildcard",
            RtKind::NotFound => "/not_found",
            RtKind::Normal => self.route.as_str(),
        };

        let cache_key = match kind {
            RtKind::Wildcard => global!("dirs.handler", route_name),
            RtKind::NotFound => global!("dirs.handler", route_name),
            RtKind::Normal => global!("dirs.cache", route_name),
        };

        let fn_name = match kind {
            RtKind::Wildcard => "wildcard",
            RtKind::NotFound => "not_found",
            RtKind::Normal => self.fn_name.as_str(),
        };

        self.route = route_name.into();
        self.fn_name = fn_name.replace("/", "_").replace(".", "_d").into();

        md5.update(&self.route);
        md5.update(&self.fn_name);
        md5.update(&self.fn_body);

        self.created = now;
        self.expires = now + Duration::hours(3);
        self.cache = Path::new(&cache_key).to_owned();
        self.hash = const_hex::encode(md5.finalize());

        return self;
    }

    // save functions that expired or dont exist
    pub async fn save(&mut self, kind: RtKind) -> RtIndex {
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

        if let Err(err) = write(self.cache.to_owned(), encoded).await {
            tracing::error!(err = string!(err), "Error writing route");
            std::process::exit(1);
        }

        ROUTES_INDEX.lock().await.insert(
            self.hash.to_owned(),
            RouteContainer {
                inner: self.clone(),
                present_in_current_update: true,
            },
        );

        return (self.hash.to_owned(), take(self));
    }

    pub async fn get(key: &str) -> Result<Route, Error> {
        let key = match key {
            "/" => global!("dirs.cache", "/index"),
            _ => global!("dirs.cache", key),
        };

        let files = HashMap::from([
            ("not_found", global!("dirs.handler", "/not_found")),
            ("wildcard", global!("dirs.handler", "/wildcard")),
            ("server_error", global!("dirs.handler", "/internal_err")),
        ]);

        let page_exists = |key| match key {
            "not_found" => file_exists!(&files.get("not_found").unwrap()),
            "wildcard" => file_exists!(&files.get("wildcard").unwrap()),
            _ => false,
        };

        let bytes = match read(&key).await {
            Ok(contents) => contents,
            Err(err) => {
                if page_exists("not_found") {
                    read(files.get("not_found").unwrap()).await?
                } else if page_exists("wildcard") {
                    read(files.get("wildcard").unwrap()).await?
                } else {
                    return Err(anyhow!(err));
                }
            }
        };

        Ok(ron::de::from_bytes(&bytes)?)
    }

    pub fn construct_fn(&self) -> String {
        let args = match self.args.to_owned() {
            Some(args) => match args.len() {
                0 => String::new(),
                1 => args[0].to_string(),
                _ => args.join(", "),
            },
            None => "".into(),
        };

        format!("fn {}({args}){{{}}}", self.fn_name, self.fn_body)
    }
}

pub mod prelude {
    pub use super::Route;
}
