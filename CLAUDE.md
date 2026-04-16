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
- `crates/roon-mcp` — MCP server (15 tools, stdio + SSE transport)
- `crates/roon-cli` — CLI tool (one-shot commands, `roon` binary)

## Conventions

- Error handling: `thiserror` in library crates, `anyhow` in the binary crate
- Async runtime: tokio
- Config: TOML file with env var overrides (prefix `ROON_HUB_`)
- Commit messages in English
- Protocol reference implementation: `/home/shin1ohno/ManagedProjects/node-roon-api/`

## Roon Core (実機テスト)

- IP: `192.168.1.20`, port: `9330` (name: "home")
- MCP direct connect: `--host 192.168.1.20 --port 9330`
- Discovery も同じネットワークで動作確認済み

## MCP Server

- MCP 設定は `.mcp.json`（プロジェクトルート）に置く — `.claude/settings.local.json` ではない
- stdio server はスタートアップ sleep 禁止（MCP クライアントのタイムアウトを引き起こす）
- `cargo run` ではなくビルド済みバイナリのパスを指定（コンパイル遅延回避）

## Docker

```sh
docker build -f crates/roon-hub/Dockerfile -t roon-hub .
docker build -f crates/roon-mcp/Dockerfile -t roon-mcp .
# --net=host required for SOOD multicast discovery
```

## Versioning / Release

All crates bump together in a unified release. There are three categories of version strings that MUST stay in sync:

1. `[package]` version in each crate's `Cargo.toml`
2. Path dependency version constraints (e.g., `roon-api = { path = "...", version = "X.Y.Z" }`)
3. Roon Extension `display_version` (3rd arg to `RoonClientBuilder::new`) in application source (`roon-cli/src/connect.rs`, `roon-mcp/src/main.rs`)

**Always use `/bump-version <new-version>` to bump.** The skill updates all three categories atomically, runs build/test/clippy, and stages the diff for review. Never edit versions by hand — it's easy to miss one of the three categories and ship inconsistent metadata.

After the skill stages the diff:
```sh
git commit
git tag v<version> && git push origin v<version>
```
Tag push triggers cargo-dist (GitHub Releases) and publish-crates (crates.io) workflows automatically.
