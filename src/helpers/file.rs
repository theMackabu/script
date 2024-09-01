use colored::Colorize;
use macros_rs::fmt::{crashln, string};
use std::{env, fs, path::PathBuf};

pub fn cwd() -> PathBuf {
    match env::current_dir() {
        Ok(path) => path,
        Err(_) => crashln!("Unable to find current working directory"),
    }
}

pub fn read_toml<T: serde::de::DeserializeOwned>(path: String) -> T {
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => crashln!("Cannot find {path}.\n{}", string!(err).white()),
    };

    match toml::from_str(&contents).map_err(|err| string!(err)) {
        Ok(parsed) => parsed,
        Err(err) => crashln!("Cannot parse {path}.\n{}", err.white()),
    }
}
