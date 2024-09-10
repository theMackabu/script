#[macro_export]
macro_rules! send {
    ($req:ident->$res:expr) => {{
        let (body, content_type, status_code) = $res;
        log::info!(
            method = $req.method().to_string(),
            status = status_code.to_string(),
            content = content_type.to_string(),
            "request '{}'",
            $req.uri()
        );
        return Ok(HttpResponse::build(status_code).content_type(content_type).body(body));
    }};
}

#[macro_export]
macro_rules! error {
    ($req:ident->$err:ident@$url:expr) => {{
        let body = Message {
            error: "Function Not Found",
            code: StatusCode::NOT_FOUND.as_u16(),
            message: format!("Have you created the <code>{}</code> route?", $url),
            note: "You can add <code>* {}</code> or <code>404 {}</code> routes as well",
        };

        log::error!(err = $err.to_string(), "Error finding route");
        send!($req->(body.render().unwrap(), ContentType::html(), StatusCode::NOT_FOUND))
    }};
}
