use rhai::{plugin::*, Map};

#[export_module]
pub mod json {
    pub fn dump<'s>(object: Dynamic) -> String {
        match serde_json::to_string(&object) {
            Ok(result) => result,
            Err(err) => err.to_string(),
        }
    }

    #[rhai_fn(global, return_raw, name = "parse")]
    pub fn parse<'s>(json: String) -> Result<Map, Box<EvalAltResult>> {
        match serde_json::from_str(&json) {
            Ok(map) => Ok(map),
            Err(err) => Err(err.to_string().into()),
        }
    }
}
