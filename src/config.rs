use crate::{helpers::prelude::*, structs::config::*};

use colored::Colorize;
use pickledb::SerializationMethod;
use std::fs;

use macros_rs::{
    fmt::{crashln, string},
    fs::file_exists,
};

pub fn read() -> Config {
    let config_path = format!("config.toml");

    if !file_exists!(&config_path) {
        let config = Config {
            env: None,
            database: None,
            workers: vec!["app.routes".into()],
            settings: Settings {
                cache: string!(".script/cache"),
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

    read_toml(config_path)
}

impl Config {
    pub fn get_address(&self) -> (String, u16) { (self.settings.address.clone(), self.settings.port.clone()) }

    pub fn kv_serialization_method(&self) -> Option<SerializationMethod> {
        let database = self.database.clone()?.kv?;

        match &*database.method {
            "json" | "default" => Some(SerializationMethod::Json),
            "yaml" | "yml" => Some(SerializationMethod::Yaml),
            "binary" | "bin" => Some(SerializationMethod::Bin),
            _ => Some(SerializationMethod::Bin),
        }
    }
}
