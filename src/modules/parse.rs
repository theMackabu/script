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

pub mod array {
    pub fn join(array: &mut rhai::Array) -> String { array.iter().map(|x| x.to_string()).collect::<Vec<String>>().join("") }

    pub fn join_separator(array: &mut rhai::Array, separator: &str) -> String { array.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(separator) }

    pub fn pad(arr: &mut rhai::Array, count: i64, value: rhai::Dynamic) -> rhai::Array {
        let mut new_arr = arr.clone();
        for _ in 0..count {
            new_arr.push(value.clone());
        }
        new_arr
    }
}

pub mod string {
    pub fn repeat(s: &str, count: i64) -> String {
        let mut result = String::with_capacity((s.len() as i64 * count) as usize);
        for _ in 0..count {
            result.push_str(s);
        }
        result
    }
}
