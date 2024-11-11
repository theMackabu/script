use rhai::plugin::*;
use std::process::Command;
use std::{collections::HashMap, path::PathBuf};

#[export_module]
pub mod cmd {
    #[rhai_fn(return_raw)]
    pub fn start(command: &str) -> Result<Dynamic, Box<EvalAltResult>> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or("Empty command")?;
        let args: Vec<&str> = parts.collect();
        let output = Command::new(program).args(&args).output().map_err(|e| e.to_string())?;

        let mut result = HashMap::new();

        result.insert("stdout", String::from_utf8_lossy(&output.stdout).to_string());
        result.insert("stderr", String::from_utf8_lossy(&output.stderr).to_string());
        result.insert("status", output.status.code().unwrap_or(-1).to_string());

        Ok(Dynamic::from(result))
    }

    #[rhai_fn(return_raw)]
    pub fn run(command: &str) -> Result<String, Box<EvalAltResult>> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or("Empty command")?;
        let args: Vec<&str> = parts.collect();
        let output = Command::new(program).args(&args).output().map_err(|e| e.to_string())?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string().into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[rhai_fn(return_raw)]
    pub fn command_exists(command: &str) -> Result<bool, Box<EvalAltResult>> {
        let paths = std::env::var_os("PATH").ok_or("PATH environment variable not found")?;

        let exe_extensions: Vec<String> = if cfg!(target_os = "windows") {
            std::env::var("PATHEXT").unwrap_or(".EXE;.CMD;.BAT".to_string()).split(';').map(|ext| ext.to_lowercase()).collect()
        } else {
            vec!["".to_string()]
        };

        for dir in std::env::split_paths(&paths) {
            for ext in &exe_extensions {
                let mut file_path = PathBuf::from(&dir);
                let mut command_with_ext = command.to_string();
                command_with_ext.push_str(ext);
                file_path.push(&command_with_ext);

                if file_path.is_file() {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    #[rhai_fn(return_raw)]
    pub fn pwd() -> Result<String, Box<EvalAltResult>> { std::env::current_dir().map(|p| p.to_string_lossy().to_string()).map_err(|e| e.to_string().into()) }
}
