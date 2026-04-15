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
- [ ] Connection manager (state machine, reconnection) — deferred to Phase 8

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

- [ ] TOML config + env var overrides
- [ ] MQTT bridge (rumqttc)
- [ ] Zone state manager + routing table
- [ ] Command router (device event → zone → transport command)
- [ ] Graceful shutdown

### Phase 8: Hardening

- [ ] Reconnection logic (exponential backoff)
- [ ] Seek position throttling
- [ ] RPi memory profiling
- [ ] CI pipeline (aarch64 cross-compile)
- [ ] Systemd service file

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
