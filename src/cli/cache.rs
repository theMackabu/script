use chrono::{DateTime, Duration, Utc};
use colored::{Color, ColoredString, Colorize};
use macros_rs::fmt::crashln;

use crate::{
    routes::{self, parse, Route},
    structs::config::Config,
};

fn format_time(dt: DateTime<Utc>) -> ColoredString {
    let now = Utc::now();
    let duration = dt.signed_duration_since(now);

    let (duration, prefix, suffix) = if duration.num_seconds() >= 0 {
        (duration, "expires in ", "")
    } else {
        (-duration, "expired ", " ago")
    };

    let (formatted, color) = match duration {
        d if d < Duration::hours(1) => {
            if d < Duration::minutes(1) {
                (format!("{}s", d.num_seconds()), Color::Red)
            } else {
                (format!("{}m", d.num_minutes()), Color::Red)
            }
        }
        d if d < Duration::hours(2) => {
            let hours = d.num_hours();
            let mins = d.num_minutes() % 60;
            (if mins == 0 { format!("{}h", hours) } else { format!("{}h {}m", hours, mins) }, Color::Yellow)
        }
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

pub fn clean(config: Config) {
    match std::fs::remove_dir_all(config.settings.cache) {
        Ok(_) => println!("{}", "cleaned route cache".green()),
        Err(_) => crashln!("{}", "route cache does not exist, cannot remove".yellow()),
    };
}

pub async fn build(config: Config) {
    // follow the same system later that main.rs will use for import system
    let filename = &config.workers.get(0).unwrap();

    let contents = match std::fs::read_to_string(&filename) {
        Ok(contents) => contents,
        Err(err) => crashln!("Error reading script file: {}\n{}", filename.to_string_lossy(), err),
    };

    // move error handling here
    parse::try_parse(&contents).await;

    // have error message as well in red with crashln
    // make it say rebuilt route cache when files exist
    println!("{}", "built route cache".green());
    println!("you can view all the cached routes with 'script cache list'");
}

pub async fn list(config: Config) {
    let mut internal_routes: Vec<Route> = Vec::new();
    // add error handling
    let index = routes::routes_index(config.settings.cache).await.unwrap();
    let index = index.lock().await;

    let mut sorted_items: Vec<_> = index.iter().map(|item| item.to_owned()).collect();
    sorted_items.sort_by(|a, b| a.fn_name.cmp(&b.fn_name));

    for item in sorted_items {
        match item.fn_name.as_str() {
            "not_found" | "wildcard" => internal_routes.push(item),
            _ => print_item(item, false),
        };
    }

    for item in internal_routes.into_iter() {
        print_item(item, true)
    }
}
