pub use askama::Template;

#[derive(Template)]
#[template(path = "error.html")]
pub struct ServerError {
    pub error: String,
    pub context: Vec<(String, String)>,
}

#[derive(Template)]
#[template(path = "message.html")]
pub struct Message<'a> {
    pub code: u16,
    pub note: &'a str,
    pub error: &'a str,
    pub message: String,
}
