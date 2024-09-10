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

use tokio::{
    fs::{read, write},
    sync::mpsc,
};

use std::{
    collections::{HashMap, HashSet, VecDeque},
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
pub type RtSearchIndex = Option<(Route, Vec<String>)>;
pub type RtGlobalIndex = Arc<Mutex<DashMap<String, RouteContainer>>>;

pub enum RtKind {
    Normal,
    Wildcard,
    NotFound,
}

pub struct RouteContainer {
    pub inner: Route,
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

pub async fn routes_index(root_dir: String) -> Result<Vec<Route>, Error> {
    let mut index = Vec::new();
    let mut dirs_to_visit = VecDeque::new();
    dirs_to_visit.push_back(PathBuf::from(root_dir));

    while let Some(dir) = dirs_to_visit.pop_front() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                dirs_to_visit.push_back(path);
            } else if path.extension().map_or(false, |ext| ext == "route") {
                let route_container = Route::from_path(path).await?;
                index.push(route_container);
            }
        }
    }

    Ok(index)
}

async fn get_fallback_route() -> Option<(Route, Vec<String>)> {
    let fallback_routes = [("not_found", "__handler_not_found"), ("wildcard", "__handler_wildcard")];

    let page_exists = |key| match key {
        "not_found" => file_exists!(&global!("dirs.handler", "/not_found")),
        "wildcard" => file_exists!(&global!("dirs.handler", "/wildcard")),
        _ => false,
    };

    for (page, handler) in fallback_routes {
        if page_exists(page) {
            if let Ok(route) = Route::get(handler).await {
                return Some((route, vec![]));
            }
        }
    }

    None
}

async fn match_route(route_template: &str, placeholders: &[RtData], url: &str) -> Option<Vec<String>> {
    let route_segments: Vec<String> = route_template.split('/').map(String::from).collect();
    let url_segments: Vec<String> = url.split('/').map(String::from).collect();

    if route_segments.len() != url_segments.len() {
        return None;
    }

    let segments_count = route_segments.len();
    let (tx, mut rx) = mpsc::channel(route_segments.len());

    for (i, (route_segment, url_segment)) in route_segments.into_iter().zip(url_segments.into_iter()).enumerate() {
        let tx = tx.clone();
        let placeholders = placeholders.to_vec();
        tokio::spawn(async move {
            let result = match_segment(&route_segment, &url_segment, &placeholders);
            let _ = tx.send((i, result)).await;
        });
    }

    let mut matched_placeholders = vec![None; segments_count];
    let mut matched_count = 0;

    while let Some((index, result)) = rx.recv().await {
        match result {
            Some(values) => {
                matched_placeholders[index] = Some(values);
                matched_count += 1;
                if matched_count == segments_count {
                    break;
                }
            }
            None => return None,
        }
    }

    Some(matched_placeholders.into_iter().flatten().flatten().collect())
}

fn match_segment(route_segment: &str, url_segment: &str, placeholders: &[RtData]) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut route_parts = route_segment.split('{');
    let mut url_chars = url_segment.chars().peekable();

    if let Some(prefix) = route_parts.next() {
        if !url_segment.starts_with(prefix) {
            return None;
        }
        for _ in 0..prefix.len() {
            url_chars.next();
        }
    }

    for part in route_parts {
        let (placeholder, suffix) = part.split_once('}')?;
        if !placeholders.contains(&RtData::from(placeholder)) {
            return None;
        }

        let mut value = String::new();
        while let Some(&c) = url_chars.peek() {
            if suffix.starts_with(c) {
                break;
            }
            value.push(url_chars.next().unwrap());
        }
        result.push(value);

        for expected_char in suffix.chars() {
            if url_chars.next() != Some(expected_char) {
                return None;
            }
        }
    }

    if url_chars.next().is_some() {
        None
    } else {
        Some(result)
    }
}

impl Route {
    pub fn default() -> Self { Default::default() }

    pub async fn search_for(url: String) -> RtSearchIndex {
        let index = ROUTES_INDEX.lock().await;
        let (tx, mut rx) = mpsc::channel::<RtSearchIndex>(index.len());

        for entry in index.iter() {
            let (_, route_container) = entry.pair();

            let route_template = route_container.inner.route.to_owned();
            let placeholders = route_container.inner.args.to_owned().unwrap_or_default();
            let route_clone = route_container.inner.to_owned();

            let tx = tx.to_owned();
            let url = url.to_owned();

            tokio::spawn(async move {
                if let Some(matched_values) = match_route(&route_template, &placeholders, &url).await {
                    let _ = tx.send(Some((route_clone, matched_values))).await;
                }
            });
        }

        drop(tx);
        rx.recv().await.flatten().or_else(|| futures::executor::block_on(get_fallback_route()))
    }

    pub async fn cleanup() -> std::io::Result<()> {
        let cache_dir = PathBuf::from(global!("base.cache"));
        let routes = ROUTES_INDEX.lock().await;

        log::trace!("Cache directory: {:?}", cache_dir);

        let valid_cache_files: HashSet<PathBuf> = routes
            .iter()
            .map(|item| {
                let path = item.value().inner.cache.clone();
                log::trace!("Valid cache file: {:?}", path);
                path
            })
            .collect();

        for entry in WalkDir::new(&cache_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if path.is_file() {
                log::trace!("Checking file: {:?}", path);

                let should_keep = valid_cache_files.iter().any(|valid_path| {
                    let paths_match = path == *valid_path;
                    log::trace!("Comparing {:?} with {:?}: {}", path, valid_path, paths_match);
                    paths_match
                });

                if !should_keep {
                    log::debug!("Deleting file: {:?}", path);
                    remove_file(&path)?;
                } else {
                    log::debug!("Keeping file: {:?}", path);
                }
            } else if entry.file_type().is_dir() {
                if read_dir(path.to_owned())?.next().is_none() {
                    log::debug!("Removing empty directory: {:?}", path);
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

    pub fn cache(&mut self, kind: &RtKind) -> (&Self, DateTime<Utc>) {
        let current_time = Utc::now();
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

        self.cache = Path::new(&cache_key).to_owned();
        self.hash = const_hex::encode(md5.finalize());

        return (self, current_time);
    }

    // save functions that expired or dont exist
    pub async fn save(&mut self, kind: RtKind) -> RtIndex {
        let current_time = self.cache(&kind).1;
        let current_route = self.cache.to_owned();

        // add error handling
        // make sure it wont error if cached route doesnt exist somehow
        if let Ok(route) = Route::from_path(current_route).await {
            if self.hash == route.hash {
                if current_time <= route.expires {
                    self.created = route.created;
                    self.expires = route.expires;
                    return (self.hash.to_owned(), take(self));
                }
            }
        }

        self.created = current_time;
        self.expires = current_time + Duration::hours(3);

        if let Some(parent) = self.cache.parent() {
            if !parent.exists() {
                // add error handling
                create_dir_all(parent).unwrap();
            }
        }

        let encoded = match ron::ser::to_string(&self) {
            Ok(contents) => contents,
            Err(err) => {
                log::error!(err = string!(err), "Cannot encode route");
                std::process::exit(1);
            }
        };

        if let Err(err) = write(self.cache.to_owned(), encoded).await {
            log::error!(err = string!(err), "Error writing route");
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

    pub async fn from_path(path: PathBuf) -> Result<Route, Error> {
        let bytes = match read(&path).await {
            Ok(contents) => contents,
            Err(err) => return Err(anyhow!(err)),
        };

        Ok(ron::de::from_bytes(&bytes)?)
    }

    pub async fn get(key: &str) -> Result<Route, Error> {
        let key = match key {
            "/" => global!("dirs.cache", "/index"),
            "__handler_not_found" => global!("dirs.handler", "/not_found"),
            "__handler_wildcard" => global!("dirs.handler", "/wildcard"),
            _ => global!("dirs.cache", key),
        };

        Ok(Route::from_path(key.into()).await?)
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
    pub use super::parse;
    pub use super::Route;
}
