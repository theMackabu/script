use crate::structs::config::Config;
use global_placeholders::init;
use macros_rs::fs::{file_exists, folder_exists};
use panic::setup_panic;
use std::fs::create_dir_all;

pub fn init(cli: &crate::Cli) -> Config {
    if !file_exists!(&cli.config) {
        Config::new().set_path(&format!("{}.tmp", cli.config)).write_example()
    }

    let mut config = Config::new().set_path(&cli.config).read();

    if let Some(port) = cli.port.to_owned() {
        config.override_port(port)
    }

    if let Some(cache) = cli.cache.to_owned() {
        config.override_cache(cache)
    }

    if let Some(address) = cli.address.to_owned() {
        config.override_address(address)
    }

    let cache_dir = format!("{}/cache", config.settings.cache);

    if !folder_exists!(&cache_dir) {
        create_dir_all(&cache_dir).unwrap();
        log::info!("created cached dir");
    }

    init!("base.cache", config.settings.cache);
    init!("base.handler", format!("{}/handler", config.settings.cache));

    init!("dirs.cache", format!("{}/cache{{}}.route", config.settings.cache));
    init!("dirs.handler", format!("{}/handler{{}}.route", config.settings.cache));
    init!("dirs.cache.index", format!("{}/routes.toml", config.settings.cache));
    init!("dirs.cache.hash", format!("{}/hashes.toml", config.settings.cache));

    setup_panic! {
        name: "Script Web Engine",
        short_name: "script",
        version: env!("CARGO_PKG_VERSION"),
        repository: "https://github.com/themackabu/script",
        messages: {
            colors: (Color::Red, Color::White, Color::Green),
            head: "Well, this is embarrassing. %(name) v%(version) had a problem and crashed. \nTo help us diagnose the problem you can send us a crash report\n",
            body: "We have generated a report file at \"%(file_path)\". \nSubmit an issue or email with the subject of \"%(name) v%(version) crash report\" and include the report as an attachment at %(repository).\n",
            footer: "We take privacy seriously, and do not perform any automated error collection. \nIn order to improve the software, we rely on people to submit reports. Thank you!"
        }
    };

    return config;
}
