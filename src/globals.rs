use crate::config;
use global_placeholders::init;
use macros_rs::fs::folder_exists;
use std::fs::create_dir_all;

pub fn init() {
    let config = config::read();

    if !folder_exists!(&config.settings.cache) {
        create_dir_all(&config.settings.cache).unwrap();
        tracing::info!("created cached dir");
    }

    init!("dirs.cache", format!("{}{{}}.route", config.settings.cache));
}
