use macros_rs::fmt::str;
use rhai::plugin::*;
use std::path::Path;

#[export_module]
pub mod exists {
    #[rhai_fn(global, return_raw, name = "folder")]
    pub fn folder(dir_name: String) -> Result<bool, Box<EvalAltResult>> { Ok(Path::new(str!(dir_name)).is_dir()) }
    #[rhai_fn(global, return_raw, name = "file")]
    pub fn file(file_name: String) -> Result<bool, Box<EvalAltResult>> { Ok(Path::new(str!(file_name)).exists()) }
}
