use colored::Colorize;
use macros_rs::{crashln, str, string};
use rhai::plugin::*;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn cwd() -> PathBuf {
    match env::current_dir() {
        Ok(path) => path,
        Err(_) => crashln!("Unable to find current working directory"),
    }
}

#[export_module]
pub mod exists {
    #[rhai_fn(global, return_raw, name = "folder")]
    pub fn folder(dir_name: String) -> Result<bool, Box<EvalAltResult>> { Ok(Path::new(str!(dir_name)).is_dir()) }
    #[rhai_fn(global, return_raw, name = "file")]
    pub fn file(file_name: String) -> Result<bool, Box<EvalAltResult>> { Ok(Path::new(str!(file_name)).exists()) }
}

pub fn read<T: serde::de::DeserializeOwned>(path: String) -> T {
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => crashln!("Cannot find {path}.\n{}", string!(err).white()),
    };

    match toml::from_str(&contents).map_err(|err| string!(err)) {
        Ok(parsed) => parsed,
        Err(err) => crashln!("Cannot parse {path}.\n{}", err.white()),
    }
}
