use chrono::{DateTime, Duration, Utc};
use colored::{Color, ColoredString, Colorize};
use macros_rs::fmt::crashln;

use crate::{
    routes::{parse, Route, ROUTES_INDEX},
    structs::config::Config,
};

fn format_time(dt: DateTime<Utc>) -> ColoredString {
    let now = Utc::now();
    let duration = dt - now;

    let (duration, prefix, suffix) = if duration.num_seconds() >= 0 {
        (duration, "expires in ", "")
    } else {
        (-duration, "expired ", " ago")
    };

    let (formatted, color) = match duration {
        d if d < Duration::minutes(1) => (format!("{}s", d.num_seconds()), Color::Red),
        d if d < Duration::hours(1) => (format!("{}m", d.num_minutes()), Color::Yellow),
        d => {
            let hours = d.num_hours();
            let mins = d.num_minutes() % 60;
            (if mins == 0 { format!("{}h", hours) } else { format!("{}h {}m", hours, mins) }, Color::Green)
        }
    };

    format!("{}{}{}", prefix, formatted.bold(), suffix).color(color)
}

fn print_item(item: Route, internal: bool) {
    let star = "*".bold();
    let dash = "-".bold();

    let expiry = format_time(item.expires);
    let fn_name = item.fn_name.bright_cyan().bold();
    let route = item.route.replace("index", "").cyan();
    let file_path = format!("({})", item.cache.to_string_lossy()).white();

    if internal {
        println!("{star} {fn_name} {dash} {expiry} {file_path}");
    } else {
        println!("{star} {fn_name} {route} {dash} {expiry} {file_path}");
    }
}

pub async fn list(config: Config) {
    let filename = &config.workers.get(0).unwrap();
    let mut internal_routes: Vec<Route> = Vec::new();

    let contents = match std::fs::read_to_string(&filename) {
        Ok(contents) => contents,
        Err(err) => crashln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err),
    };

    // move error handling here
    parse::try_parse(&contents).await;

    let index = ROUTES_INDEX.lock().await;

    for item in index.iter() {
        let item = item.inner.to_owned();
        match item.fn_name.as_str() {
            "not_found" | "wildcard" => internal_routes.push(item),
            _ => print_item(item, false),
        };
    }

    for item in internal_routes.into_iter() {
        print_item(item, true)
    }
}
