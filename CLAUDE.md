# roon-rs

Rust SDK for Roon audio system's proprietary protocols (SOOD discovery + MOO RPC). `roon-api` is a standalone library any Rust program can use to control Roon. `roon-hub` is one consumer that bridges Roon to MQTT.

## Project Navigation

- Master plan + progress: `docs/PLAN.md` (read this first every session)
- Protocol specs: `docs/protocols/{SOOD,MOO}.md`
- Design decisions: `docs/ADR/`
- Cognee dataset: `roon_protocols`

## Build & Test

```sh
cargo build --workspace
cargo test --workspace
```

## Crate Structure

- `crates/roon-sood` — SOOD UDP discovery protocol
- `crates/roon-moo` — MOO WebSocket RPC protocol
- `crates/roon-api` — Roon SDK (discovery, registration, transport, browse)
- `crates/roon-hub` — Hub binary (MQTT bridge + device routing)

## Conventions

- Error handling: `thiserror` in library crates, `anyhow` in the binary crate
- Async runtime: tokio
- Config: TOML file with env var overrides (prefix `ROON_HUB_`)
- Commit messages in English
- Protocol reference implementation: `/home/shin1ohno/ManagedProjects/node-roon-api/`
