pub mod structs;

use crate::file::{self, exists};
use colored::Colorize;
use macros_rs::fmt::{crashln, string};
use std::fs;
use structs::{Config, Settings};

pub fn read() -> Config {
    let config_path = format!("config.toml");

    if !exists::file(config_path.clone()).unwrap() {
        let config = Config {
            env: None,
            database: None,
            workers: vec!["app.routes".into()],
            settings: Settings {
                address: string!("127.0.0.1"),
                port: 3500,
            },
        };

        let contents = match toml::to_string(&config) {
            Ok(contents) => contents,
            Err(err) => crashln!("Cannot parse config.\n{}", string!(err).white()),
        };

        if let Err(err) = fs::write(&config_path, contents) {
            crashln!("Error writing config.\n{}", string!(err).white())
        }
        tracing::info!(path = config_path, created = true, "config");
    }

    file::read(config_path)
}

impl Config {
    pub fn get_address(&self) -> (String, u16) { (self.settings.address.clone(), self.settings.port.clone()) }
}
