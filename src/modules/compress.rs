use ::tar::{Archive, Builder};
use flate2::Compression;
use rhai::plugin::*;
use std::{fs::File, path::Path};

#[export_module]
pub mod tar {
    #[rhai_fn(global, return_raw, name = "extract")]
    pub fn extract(filepath: &str) -> Result<Dynamic, Box<EvalAltResult>> {
        let file = File::open(filepath).map_err(|e| format!("Failed to open tar file: {}", e))?;

        let mut archive = Archive::new(file);
        let extract_dir = Path::new(filepath).parent().unwrap_or(Path::new("."));

        archive.unpack(extract_dir).map_err(|e| format!("Failed to extract tar: {}", e))?;
        Ok(Dynamic::from(extract_dir.to_string_lossy().to_string()))
    }

    #[rhai_fn(global, return_raw, name = "compress")]
    pub fn compress(files: Vec<String>, output: &str) -> Result<Dynamic, Box<EvalAltResult>> {
        let file = File::create(output).map_err(|e| format!("Failed to create tar file: {}", e))?;

        let encoder = flate2::write::GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);

        for path in files {
            let path = Path::new(&path);
            builder.append_path(path).map_err(|e| format!("Failed to add file to tar: {}", e))?;
        }

        builder.finish().map_err(|e| format!("Failed to finalize tar: {}", e))?;
        Ok(Dynamic::from(output.to_string()))
    }
}
