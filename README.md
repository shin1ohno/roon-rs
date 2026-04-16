# roon-rs

Rust SDK and tools for [Roon](https://roonlabs.com/)'s proprietary protocols (SOOD discovery + MOO RPC).

- **`roon-api`** — Standalone SDK. Any Rust program can add it as a dependency to discover, connect to, and control a Roon Core.
- **`roon-cli`** — Command-line tool (`roon`) for controlling Roon from a terminal.
- **`roon-mcp`** — MCP server exposing Roon as tools for AI assistants (Claude Code, etc.).
- **`roon-hub`** — MQTT bridge binary (not published to crates.io).

## Install the CLI

### From crates.io

```sh
cargo install roon-cli
```

### From GitHub Releases (pre-built binaries)

```sh
# roon-cli (the `roon` command)
curl -LsSf https://github.com/shin1ohno/roon-rs/releases/latest/download/roon-cli-installer.sh | sh

# roon-mcp (MCP server)
curl -LsSf https://github.com/shin1ohno/roon-rs/releases/latest/download/roon-mcp-installer.sh | sh

# Windows (PowerShell)
powershell -c "irm https://github.com/shin1ohno/roon-rs/releases/latest/download/roon-cli-installer.ps1 | iex"
```

Supported targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.

## Install the MCP server

```sh
cargo install roon-mcp
# or grab a pre-built binary from GitHub Releases (see above).
```

Run `roon-mcp --transport stdio` (default) or `--transport sse --http-port 8080` for SSE/HTTP. See `crates/roon-mcp/` for details.

## Quick Start

```sh
# 1. Discover Roon Cores on your network and set the default server.
#    Approve "roon-rs CLI" in Roon Settings > Extensions when prompted.
roon discover

# 2. Pick a default zone.
roon zone

# 3. Play.
roon play                          # resume playback
roon play -A "Miles Davis"         # search artist and play
roon play -a "Kind of Blue"        # search album and play
roon play -A "Miles Davis" -s      # shuffle all tracks from an artist
roon pause / stop / next / previous

# 4. Inspect.
roon status                        # current zone's now playing
roon zones --json                  # all zones as JSON
```

Full command list: `roon --help`.

## Use the SDK

```toml
[dependencies]
roon-api = "0.1"
tokio = { version = "1", features = ["full"] }
```

```rust
use roon_api::{RoonClientBuilder, RoonEvent, ControlAction, FileStateStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = RoonClientBuilder::new(
        "com.example.myapp",
        "My App",
        "0.1.0",
        "My Name",
        "me@example.com",
    )
    .token_store(FileStateStore::new("tokens.json"))
    .require_transport()
    .build()?;

    let core = client.connect("192.168.1.20", 9330).await?;
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    for z in &zones {
        println!("{}: {:?}", z.display_name, z.state);
    }
    Ok(())
}
```

## Build from source

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --tests
```

## Protocol Documentation

- [SOOD (discovery)](docs/protocols/SOOD.md)
- [MOO (WebSocket RPC)](docs/protocols/MOO.md)
- [Implementation plan](docs/PLAN.md)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Disclaimer

This project is not affiliated with or endorsed by Roon Labs. "Roon" is a trademark of Roon Labs LLC.
