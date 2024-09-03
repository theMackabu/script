# barebones http scripting

## Overview

Define routes on runtime and execute scripts associated with each route. The server supports fetching data, for proxy purposes.

## Usage

```rust
// the index route
index {
   text(":3\nwelcome to the root")
}

// this is /hello
hello {
   text("Hello World!")
}

// get data from another website, then return as json
tests/fetch {
   json(http::get("https://httpbin.org/json").json())
}

// route placeholders
#[route("/example/{id}")]
example(id) {
   text("base: " + id)
}
```

For more syntax, check out `app.routes`

```bash
# Start the server
script start <config_path> # (default config.toml)
```

For more commands, check out `script --help`

### Installation

Pre-built binaries for Linux, MacOS, and Windows can be found on the [releases](releases) page.

Install from crates.io using `cargo install script`

#### Building

- Clone the project
- Open a terminal in the project folder
- Check if you have cargo (Rust's package manager) installed, just type in `cargo`
- If cargo is installed, run `cargo build --release`
- Put the executable into one of your PATH entries, usually `/bin/` or `/usr/bin/`
