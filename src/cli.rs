pub mod cache;
pub mod verbose;

pub fn get_version(short: bool) -> String {
    return match short {
        true => format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        false => match env!("GIT_HASH") {
            "" => format!("{} ({}) [{}]", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"), env!("PROFILE")),
            hash => format!("{} ({} {hash}) [{}]", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"), env!("PROFILE")),
        },
    };
}
