use colored::Colorize;
use macros_rs::fmt::{crashln, string};
use std::{env, fs, io::Result, path::PathBuf};

pub fn cwd() -> PathBuf {
    match env::current_dir() {
        Ok(path) => path,
        Err(_) => crashln!("Unable to find current working directory"),
    }
}

pub fn read_toml<T: serde::de::DeserializeOwned>(path: PathBuf) -> T {
    let path_fmt = path.to_str().unwrap_or("");

    log::info!(path = path_fmt, immutable = true, "reading file");

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => crashln!("Cannot find {path_fmt}.\n{}", string!(err).white()),
    };

    match toml::from_str(&contents).map_err(|err| string!(err)) {
        Ok(parsed) => parsed,
        Err(err) => crashln!("Cannot parse {path_fmt}.\n{}", err.white()),
    }
}

pub async fn get_workers(workers: &[PathBuf]) -> Result<String> {
    let mut combined_contents = String::new();

    for worker in workers {
        match tokio::fs::read_to_string(worker).await {
            Ok(contents) => {
                combined_contents.push_str(&contents);
                combined_contents.push('\n');
            }
            Err(err) => log::error!("error reading file {}: {}", worker.display(), err),
        }
    }

    Ok(combined_contents)
}
