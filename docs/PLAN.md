# roon-rs: Implementation Plan

## Overview

Rust reimplementation of Roon's proprietary protocols (SOOD/MOO) with a Hub-and-Spoke architecture for IoT device integration via MQTT.

## Architecture

```
Roon Core <-WebSocket (MOO)-> Rust Hub (single process)
                                |
                   Zone/Group State (in-memory, typed)
                   Routing Table: Device -> Zone
                                |
                         MQTT client (1 connection)
                                |
              +-----------------+-----------------+
           Adapter A         Adapter B          Adapter C
           (Nuimo/BLE)      (StreamDeck/USB)   (HueDial/HTTP)
```

Zone switching = routing table update in memory. No connection teardown.

## Workspace Structure

```
roon-rs/
  crates/
    roon-sood/    SOOD UDP discovery protocol
    roon-moo/     MOO WebSocket RPC protocol
    roon-api/     Roon API (registry, pairing, transport, browse)
    roon-hub/     Hub binary (MQTT bridge + routing)
```

## Progress

### Phase 1: Protocol Crates (roon-sood + roon-moo)

- [x] Workspace initialization
- [x] SOOD parser + serializer
- [x] SOOD unit tests + proptest round-trip
- [ ] SOOD discovery (multicast sockets, interface polling)
- [ ] MOO parser + serializer
- [ ] MOO unit tests
- [ ] MOO connection (WebSocket + heartbeat)
- [ ] MOO subscription helper

### Phase 2: Roon API (roon-api)

- [ ] Registry handshake (info -> register -> Registered)
- [ ] Token persistence
- [ ] Pairing service (provided to Roon Core)
- [ ] Ping service
- [ ] Transport subscription (subscribe_zones)
- [ ] Transport control commands (play, pause, stop, next, previous)
- [ ] **Milestone: CLI zone listing**

### Phase 3: Hub Binary (roon-hub)

- [ ] TOML config + logging
- [ ] MQTT bridge (rumqttc)
- [ ] Zone state manager
- [ ] Routing table
- [ ] Command router (device event -> zone -> transport command)
- [ ] Graceful shutdown (CancellationToken)

### Phase 4: Hardening

- [ ] Reconnection logic (exponential backoff)
- [ ] Seek position throttling
- [ ] RPi memory profiling
- [ ] CI pipeline (aarch64 cross-compile)
- [ ] Systemd service file

## Technical Decisions

| Decision | Choice | Rationale |
|---|---|---|
| MOO response model | mpsc channel per request_id | Natural fit for CONTINUE*/COMPLETE |
| Error handling | thiserror (libs), anyhow (bin) | Structured errors + top-level convenience |
| Config | TOML + env vars | Rust standard + container-friendly |
| State sharing | Arc<RwLock<ZoneState>> | Low contention (1 writer, N readers) |
| Hub-Adapter comms | MQTT | Language-agnostic, retain/LWT, distributed hosts |

## Protocol References

- [SOOD Protocol Spec](protocols/SOOD.md)
- [MOO Protocol Spec](protocols/MOO.md)
