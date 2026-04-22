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

# 3. (Optional) Pick a default output. Used by volume/mute when --output is omitted.
#    If skipped, volume/mute fall back to the default zone's output when the zone has only one.
roon output

# 4. Play.
roon play                          # resume playback
roon play -A "Miles Davis"         # search artist and play
roon play -a "Kind of Blue"        # search album and play
roon play -A "Miles Davis" -s      # shuffle all tracks from an artist
roon pause / stop / next / previous
roon volume 30                     # uses default output (or default zone's only output)
roon mute on                       # same fallback chain

# 5. Inspect.
roon status                        # current zone's now playing
roon zones --json                  # all zones as JSON
```

Full command list: `roon --help`.

## Machine-readable integration (`watch` / `browse` / `search` / `play-item`)

These four commands emit JSON and are designed to be driven by another program (for example, a Neovim plugin or a TUI). The non-trivial idea is that browse/search state lives on the Roon Core and is keyed by a `--session` string you choose. Pass the same session key to successive calls to navigate a cursor; use different keys for independent cursors (telescope-style incremental search vs. neo-tree-style expand, for example).

### `roon watch` — streaming zone/output changes

Emits one JSON object per line (NDJSON). Default output:

```sh
roon watch                  # all events, seek throttled to 1 Hz per zone
roon watch --no-initial     # skip the one-shot initial snapshot
roon watch --seek-hz 0      # disable seek throttle (every tick)
```

Schema (`"schema": 1`):

| `event`          | fields (besides `schema` + `ts`)      | meaning                       |
| ---------------- | ------------------------------------- | ----------------------------- |
| `initial`        | `zones: [Zone]`, `outputs: [Output]`  | one-shot on start             |
| `zone_added`     | `zone: Zone`                          |                               |
| `zone_changed`   | `zone: Zone`                          |                               |
| `zone_removed`   | `zone_id: String`                     |                               |
| `zone_seeked`    | `zone_id`, `seek_position`, `queue_time_remaining` | throttled per `--seek-hz` |
| `output_added`   | `output: Output`                      |                               |
| `output_changed` | `output: Output`                      |                               |
| `output_removed` | `output_id: String`                   |                               |

Ctrl-C and a broken pipe both exit `0`. A stdout write error (other than broken pipe) exits `2`.

### `roon browse` — one-shot browse+load pair

```sh
roon browse --session nvim-neotree --hierarchy albums --pop-all
roon browse --session nvim-neotree --item-key <key>           # drill
roon browse --session nvim-neotree --offset 100 --count 50    # paginate
```

Each invocation does `browse` followed by `load` against the given session and prints the loaded list:

```json
{
  "session": "nvim-neotree",
  "list":  { "title": "Albums", "level": 1, "count": 1287, "subtitle": null, "hint": null, "image_key": null },
  "items": [ { "item_key": "4:28", "title": "In Rainbows", "subtitle": "Radiohead",
               "image_key": "...", "hint": "list", "input_prompt": null } ],
  "offset": 0,
  "total": 1287
}
```

Roon requires a `hierarchy` on every browse/load call. The first call that establishes a hierarchy caches it in `~/.config/roon-rs/sessions/<session>.toml`, so later drills on the same session do not need `--hierarchy`.

### `roon search` — thin wrapper

```sh
roon search --session nvim-telescope --input "radiohead"
```

Uses the `search` hierarchy with `pop_all=true` so each call returns fresh results. Output schema is identical to `browse`. The Roon `search` hierarchy returns header items (`hint: "header"`) mixed with result items; filter them client-side if you want a flat list.

### `roon play-item` — play / queue / start-radio

```sh
roon play-item --session nvim-neotree --item-key <key> --zone Qutest
roon play-item --session s --item-key <key> --action queue
```

`<key>` is an `item_key` from a preceding `browse`/`search` call in the same session — typically an `action_list`-hint item such as "Play Album". `play-item` drills into it to get the action list, then invokes `Play Now` / `Queue` / `Start Radio` per `--action` (default `auto` prefers `Play Now`). The Roon Core needs a zone target, so pass `--zone` / `--zone-id` or rely on the default `roon zone`. Output on success:

```json
{"ok":true,"played":{"title":"Play Now","item_key":"30:0"}}
```

On no matching action, exit code `3` and:

```json
{"error":"no matching action","available":["Go Back"]}
```

### Session semantics

- Pick your own session keys. One cursor per key; use different keys when you want independent cursors.
- Item keys are only valid within the session that produced them. Pass the same `--session` to the follow-up `browse` / `play-item` call.
- The session's hierarchy is cached on disk; delete `~/.config/roon-rs/sessions/<key>.toml` to reset.

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

## Operational Notes

### Latency boundary

- A Roon MOO RPC round-trip depends on Roon Core health. On a LAN-local Core, `Transport::change_volume` buffered-send through ACK is typically <5 ms; when the Core is mid-task (library scan, zone group re-shuffle, network-share reconnect) it can spike past 200 ms.
- SOOD discovery is multicast-based and reconverges after network changes (DHCP lease renewal, switch reboot). The discovery window is measured in seconds, not milliseconds — this is why `roon-api` caches the last `host:port` in its state store.
- Roon extension pairing is per-`extension_id`, per-Core. Reconnect after a Core restart goes through MOO re-handshake, which adds 50–500 ms on top of the TCP handshake. Consumers that care about cold-start latency should keep the `FileStateStore` token file on disk so pairing survives restarts.

### Compatibility policy

- `roon-api` is the public Rust SDK. `RoonClient`, `RoonEvent`, `ControlAction`, `Zone`, `Output`, and the `Transport` trait form the API surface.
- Today's semver rules:
  - **MINOR** — new fields on `Zone` / `Output` / event payloads (Roon's own protocol evolves this way; serde-side struct additions are additive).
  - **MAJOR** — new `RoonEvent` or `ControlAction` variant (pattern matches without `_ =>` break at compile time). Add `_ => {}` in downstream match arms if forward-compat matters now; `#[non_exhaustive]` can be revisited later.
  - **MAJOR** — MOO method signature change, builder-required-field addition.
- `roon-cli`, `roon-mcp`, and `roon-hub` are **consumers of `roon-api`**, not part of the SDK. They ship on independent version lines. A `roon-api` breaking change does not force a consumer bump — consumers are pinned to a `roon-api` minor in their own `Cargo.toml` and roll forward at their own pace.
- The `watch` / `browse` / `search` / `play-item` NDJSON output (`"schema": 1`) is a **separate contract** from the Rust SDK, aimed at non-Rust consumers (TUI, Neovim plugin). That contract evolves with its own `schema` number — new fields are additive within a schema version; incompatible changes bump `schema`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Disclaimer

This project is not affiliated with or endorsed by Roon Labs. "Roon" is a trademark of Roon Labs LLC.
