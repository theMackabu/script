use crate::{helpers::prelude::*, structs::config::*};

use colored::Colorize;
use macros_rs::fmt::{crashln, string};
use pickledb::SerializationMethod;
use std::{fs, path::PathBuf};

impl Config {
    pub fn new() -> Self {
        Self {
            config_path: "config.toml".into(),
            env: None,
            database: None,
            workers: vec!["app.routes".into()],
            settings: Settings {
                cache: string!(".script"),
                address: string!("127.0.0.1"),
                port: 3500,
            },
        }
    }

    pub fn read(&self) -> Self { read_toml(self.config_path.to_owned()) }

    pub fn write_example(&self) {
        let config_path = self.config_path.to_str().unwrap_or("");

        let contents = match toml::to_string(self) {
            Ok(contents) => contents,
            Err(err) => crashln!("Cannot parse config.\n{}", string!(err).white()),
        };

        if let Err(err) = fs::write(&self.config_path, contents) {
            crashln!("Error writing config to {config_path}.\n{}", string!(err).white())
        }

        log::info!(path = config_path, created = true, "config");
        crashln!("Failed to find config file.\n\nAn default config has been written to {config_path}\nPlease use this to setup your app config correctly.");
    }

    pub fn set_path(&mut self, config_path: &String) -> &mut Self {
        self.config_path = PathBuf::from(config_path.to_owned());
        return self;
    }

    pub fn kv_serialization_method(&self) -> Option<SerializationMethod> {
        let database = self.database.to_owned()?.kv?;

        match &*database.method {
            "json" | "default" => Some(SerializationMethod::Json),
            "yaml" | "yml" => Some(SerializationMethod::Yaml),
            "binary" | "bin" => Some(SerializationMethod::Bin),
            _ => Some(SerializationMethod::Bin),
        }
    }

    pub fn override_port(&mut self, port: u16) { self.settings.port = port; }
    pub fn override_address(&mut self, address: String) { self.settings.address = address; }
    pub fn get_address(&self) -> (String, u16) { (self.settings.address.to_owned(), self.settings.port.to_owned()) }
}
