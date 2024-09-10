use chrono::{DateTime, Duration, Utc};
use colored::{Color, ColoredString, Colorize};
use macros_rs::fmt::crashln;
use std::{fs, io, path::Path};

use crate::{
    helpers::prelude::*,
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
    let expiry = format_time(item.expires);
    let fn_name = item.fn_name.bright_cyan().bold();
    let route = item.route.replace("index", "").cyan();
    let file_path = format!("({})", item.cache.to_string_lossy()).white();

    if internal {
        println!("{STAR} {fn_name} {DASH} {expiry} {file_path}");
    } else {
        println!("{STAR} {fn_name} {route} {DASH} {expiry} {file_path}");
    }
}

// rewrite
fn is_dir_empty<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    let mut entries = fs::read_dir(&path)?;

    while let Some(entry) = entries.next() {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if !is_dir_empty(&path)? {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
    }

    Ok(true)
}

pub fn clean(config: Config) {
    // add error handling
    if is_dir_empty(&config.settings.cache).unwrap() {
        crashln!("{WARN} {}", "Route cache does not exist, cannot remove.")
    } else {
        match std::fs::remove_dir_all(config.settings.cache) {
            Ok(_) => println!("{SUCCESS} {}", "Cleaned route cache."),
            Err(err) => crashln!("{FAIL} Failed to remove cache, {err}"),
        };
    }
}

#[tokio_wrap::sync]
pub fn build(config: Config) {
    let contents = match get_workers(&config.workers).await {
        Ok(content) => content,
        Err(err) => crashln!("{FAIL} Failed to read contents, {err}"),
    };

    // move error handling here
    parse::try_parse(&contents).await;

    // have error message as well in red with crashln
    // make it say rebuilt route cache when files exist
    // migrate to global colors in globals::SUCCESS, globals::FAIL etc
    println!("{SUCCESS} {}", "Built route cache.");
    println!("{} You can view all the cached routes with 'script cache list'", "[💡]".bright_yellow());
}

#[tokio_wrap::sync]
pub fn list(config: Config) {
    let mut internal_routes: Vec<Route> = Vec::new();

    let index = match routes::routes_index(config.settings.cache).await {
        Ok(index) => index.tap(|i| i.sort_by(|a, b| a.fn_name.cmp(&b.fn_name))),
        Err(err) => crashln!("{FAIL} Failed to read cache, {err}"),
    };

    for item in index {
        match item.fn_name.as_str() {
            "not_found" | "wildcard" => internal_routes.push(item),
            _ => print_item(item, false),
        };
    }

    for item in internal_routes.into_iter() {
        print_item(item, true)
    }
}
