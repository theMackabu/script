use crate::structs::config::Config;
use global_placeholders::init;
use macros_rs::fs::folder_exists;
use std::fs::create_dir_all;

pub fn init(config: &Config) {
    let cache_dir = format!("{}/cache", config.settings.cache);

    if !folder_exists!(&cache_dir) {
        create_dir_all(&cache_dir).unwrap();
        log::info!("created cached dir");
    }

    init!("base.cache", config.settings.cache);
    init!("base.handler", format!("{}/handler", config.settings.cache));

    init!("dirs.cache", format!("{}/cache{{}}.route", config.settings.cache));
    init!("dirs.handler", format!("{}/handler{{}}.route", config.settings.cache));
    init!("dirs.cache.index", format!("{}/routes.toml", config.settings.cache));
    init!("dirs.cache.hash", format!("{}/hashes.toml", config.settings.cache));
}
