# roon-rs: Implementation Plan

## Overview

Rust SDK for Roon audio system's proprietary protocols (SOOD discovery + MOO RPC). Any Rust program can add `roon-api` as a dependency to discover, connect to, and control Roon Core. The Hub binary (`roon-hub`) is one consumer that bridges Roon to MQTT for IoT device integration.

## Architecture

```
Dependency graph:  roon-hub → roon-api → { roon-moo, roon-sood }

Any Rust program:
  use roon_api::RoonClient;
  let client = RoonClientBuilder::new(...).require_transport().build()?;
  client.start_discovery().await?;
  // events() → CorePaired → core.transport().control(zone, Play)

Hub binary (one consumer of the SDK):
  roon-api → connect to Roon → publish zone state to MQTT → subscribe device commands
```

## Workspace Structure

```
roon-rs/
  crates/
    roon-sood/    SOOD UDP discovery protocol (parser, serializer, network discovery)
    roon-moo/     MOO WebSocket RPC protocol (parser, serializer, connection layer)
    roon-api/     Roon SDK (discovery, registration, transport, browse)
    roon-hub/     Hub binary (MQTT bridge + device routing)
```

## Progress

### Phase 1: roon-moo Wire Format

- [x] MooMessage, MooVerb, MooBody types
- [x] MOO parser (parse request/response/continue, headers, JSON/binary body)
- [x] MOO serializer (wire format output)
- [x] MooError enum with thiserror
- [x] Unit tests (20) + proptest round-trip (2)
- [x] Clippy clean

### Phase 2: roon-moo Connection Layer

- [x] MooConnection (WebSocket connect, send_request, subscribe)
- [x] Subscription type (mpsc::Receiver wrapping Stream, CONTINUE*/COMPLETE)
- [x] Heartbeat (10s ping/pong, timeout disconnect)
- [x] Bidirectional dispatch (incoming REQUESTs routed to service handlers)
- [x] Integration tests (6) with mock WebSocket server

### Phase 3: roon-sood Discovery Runtime

- [x] SOOD parser + serializer (complete)
- [x] SOOD unit tests + proptest round-trip
- [x] SoodDiscovery (multicast sockets, interface polling)
- [x] DiscoveredCore broadcast channel
- [x] Adaptive scan interval (10s → 60s)

### Phase 4: roon-api SDK — Core + Registry

- [x] RoonClientBuilder + RoonClient
- [x] Core type (core_id, display_name, service accessors)
- [x] RoonEvent enum (CoreFound, CorePaired, CoreUnpaired, CoreLost)
- [x] Registry handshake (info → register → Registered)
- [x] TokenStore trait + FileTokenStore + MemoryTokenStore
- [x] Pairing service (provided to Roon Core)
- [x] Ping service
- [x] Integration tests (4) with mock Roon Core
- [x] Connection manager (ConnectionState enum with watch_connection_state())

### Phase 5: roon-api SDK — Transport Service

- [x] Transport (subscribe_zones, subscribe_outputs)
- [x] Zone, Output, NowPlaying, PlayState types
- [x] Control commands (play, pause, stop, next, previous, seek)
- [x] Volume control (change_volume, mute)
- [x] Zone settings (shuffle, loop, auto_radio)
- [x] Output grouping (group, ungroup)
- [x] Integration tests (3) with mock Roon Core + deserialization test
- [x] **Milestone: CLI zone listing + playback control with real Roon Core** (4 zones, play/pause verified)

### Phase 6: roon-api SDK — Browse Service

- [x] Browse (browse, load)
- [x] BrowseResult, BrowseList, BrowseItem types

### Phase 7: Hub Binary (roon-hub)

- [x] TOML config + env var overrides (ROON_HUB_*)
- [x] MQTT bridge (rumqttc) — publish zone state, subscribe commands
- [x] Command router (MQTT command → transport action)
- [x] Graceful shutdown (SIGINT)
- [x] Example config (roon-hub.toml.example)

### Phase 8: Hardening

- [x] Reconnection logic (exponential backoff, 1s→60s)
- [x] Seek position throttling (1/s in roon-hub)
- [x] RPi memory profiling (4.5MB RSS, stable after 30s, no leak detected)
- [x] CI pipeline (GitHub Actions: test + aarch64 cross-compile)
- [x] Systemd service file

### Phase 9: Feature Parity with Node.js Reference

- [x] Transport: convenience_switch, toggle_standby, pause_all, get_zones, get_outputs
- [x] StateStore: paired_core_id persistence with legacy format migration
- [x] Pairing state machine: pair/unpair with subscriber notification + auto-pair
- [x] connect_with_token: one-time token registration (skips info request)
- [x] Discovery: localhost detection, interface polling (5s), paired query suppression
- [x] Status service (com.roonlabs.status:1): set_status with subscriber broadcast
- [x] Image service: get_image via HTTP, image_url builder
- [x] Volume Control service (com.roonlabs.volume:1): set_volume, set_mute callbacks
- [x] Source Control service (com.roonlabs.source_control:1): standby, convenience_switch callbacks
- [x] Builder: provide_service for user-defined provided services

## Technical Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Event model | broadcast::Receiver\<RoonEvent\> | Multi-consumer, clone-able, natural for async Rust |
| Subscription | impl Stream\<Item = ZoneEvent\> (mpsc wrapping) | CONTINUE → send, COMPLETE → close sender |
| Token persistence | TokenStore trait + FileTokenStore/MemoryTokenStore | Testable + user-customizable |
| Bidirectional dispatch | Connection task routes incoming REQUESTs to service handlers | Pairing/Ping are extension-provided services |
| Connection lifecycle | State machine (Disconnected→Discovering→Connecting→Registering→Connected) | Explicit select! loop management |
| MOO response model | mpsc channel per request_id | Natural fit for CONTINUE*/COMPLETE |
| Error handling | thiserror (libs), anyhow (bin) | Structured errors + top-level convenience |
| Config | TOML + env vars | Rust standard + container-friendly |
| Hub-Adapter comms | MQTT | Language-agnostic, retain/LWT, distributed hosts |

## Protocol References

- [SOOD Protocol Spec](protocols/SOOD.md)
- [MOO Protocol Spec](protocols/MOO.md)
