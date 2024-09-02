use std::collections::HashMap;

type Config = Option<HashMap<String, String>>;

#[derive(Debug, Default)]
pub struct Route {
    pub cfg: Config,
    pub route: String,
    pub fn_name: String,
    pub fn_body: String,
    pub fn_fmt: String,
}

impl Route {
    pub fn new() -> Self { Default::default() }
}
