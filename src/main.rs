use actix_web::{get, web::Path, App, HttpServer, Responder};
use rhai::{Engine, Scope};
use std::{fs::File, io::Read, path::PathBuf, process::exit};

fn convert_to_format(input_string: &str) -> String {
    let output_string = input_string.replace("/", "_");
    output_string
}

#[get("{url:.*}")]
async fn handler(url: Path<String>) -> impl Responder {
    let engine = Engine::new();
    let mut contents = String::new();
    let mut scope = Scope::new();

    let path = convert_to_format(&url.clone());
    let filename: PathBuf = "app.routes".into();

    let mut file = match File::open(&filename) {
        Err(err) => {
            eprintln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err);
            exit(1);
        }
        Ok(file) => file,
    };

    if let Err(err) = file.read_to_string(&mut contents) {
        eprintln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err);
        exit(1);
    }

    let contents =
        if contents.starts_with("#!") {
            &contents[contents.find('\n').unwrap_or(0)..]
        } else {
            &contents[..]
        };

    let mut ast = match engine.compile(contents) {
        Ok(ast) => ast,
        Err(_) => exit(1),
    };

    ast.set_source(filename.to_string_lossy().to_string());
    scope.push("path", url.to_string());

    match engine.call_fn::<String>(&mut scope, &ast, path, ()) {
        Ok(response) => response,
        Err(err) => err.to_string(),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || App::new().service(handler);
    let addr = ("127.0.0.1", 3000);

    println!("listening on {:?}", addr);
    HttpServer::new(app).bind(addr).unwrap().run().await
}
