# SOOD Protocol Specification

**Service Oriented Object Discovery**

SOOD is a UDP-based LAN discovery protocol used by Roon to locate Roon Cores on the local network. Clients broadcast/multicast query packets; cores respond with their service details.

## Network Configuration

| Parameter        | Value                |
|------------------|----------------------|
| Port             | 9003 (UDP)           |
| Multicast group  | 239.255.90.90        |
| Multicast TTL    | 1 (link-local only)  |

Discovery packets are sent to **both** the multicast group address and the subnet broadcast address for each network interface. This dual-send strategy ensures discovery works even on networks where multicast is blocked.

## Binary Wire Format

All SOOD messages share a common binary envelope:

```
Offset  Size     Description
──────  ──────   ──────────────────────────────
0       4 bytes  Magic: ASCII "SOOD" (0x53 0x4F 0x4F 0x44)
4       1 byte   Version: 0x02
5       1 byte   Type: ASCII "Q" (query) or "R" (response)
6..end  variable TLV property list (see below)
```

Total maximum packet size: 65535 bytes (UDP datagram limit).

### TLV Property Encoding

Starting at offset 6, zero or more properties are encoded sequentially:

```
┌───────────┬────────────────────┬───────────┬───────────────────┐
│ name_len  │ name               │ value_len │ value             │
│ (1 byte)  │ (name_len bytes)   │ (2 bytes) │ (value_len bytes) │
└───────────┴────────────────────┴───────────┴───────────────────┘
```

**name_len** (1 byte, unsigned):
- Length of the property name in bytes.
- Must be > 0. A zero-length name is invalid and the parser must reject the message.

**name** (variable, UTF-8):
- The property name string.

**value_len** (2 bytes, big-endian unsigned):
- `0xFFFF` (65535): value is **null** (absent). No value bytes follow.
- `0x0000` (0): value is an empty string. No value bytes follow.
- Any other value: length of the value string in bytes.

**value** (variable, UTF-8):
- The property value string. Present only when value_len is not 0xFFFF and not 0x0000.

Properties are parsed sequentially until the end of the UDP payload. If the buffer is exhausted mid-property, the message is invalid and must be discarded.

## Message Types

### Query (`Q`)

Sent by clients seeking services on the network.

Standard query properties:

| Property             | Description                                      |
|----------------------|--------------------------------------------------|
| `query_service_id`   | UUID of the service being sought                 |
| `_tid`               | Transaction ID (UUID v4), auto-generated if absent |

### Response (`R`)

Sent by services in reply to a query.

Standard response properties:

| Property          | Description                                              |
|-------------------|----------------------------------------------------------|
| `service_id`      | UUID of the service                                      |
| `unique_id`       | Unique identifier for this specific service instance     |
| `http_port`       | TCP port for the MOO/WebSocket API endpoint              |
| `_tid`            | Echoed transaction ID from the query                     |

Additional properties observed in real Roon Core responses (always present in practice, but should be treated as optional by clients):

| Property          | Description                                                 |
|-------------------|-------------------------------------------------------------|
| `name`            | Human-readable core name (e.g., "home", "Rhein Z1 V2")      |
| `display_version` | Core version string (e.g., "2.65 (build 1648) earlyaccess") |
| `tcp_port`        | Legacy/alternate TCP port                                   |
| `https_port`      | TLS endpoint port                                           |

## Reserved Properties

These properties have protocol-level semantics and are consumed by the parser before being delivered to application code:

| Property       | Description                                                       |
|----------------|-------------------------------------------------------------------|
| `_tid`         | Transaction ID (UUID v4). Auto-generated on outgoing queries if not set. |
| `_replyaddr`   | If present, overrides the UDP source IP in `from.ip`. Removed from props before delivery. |
| `_replyport`   | If present, overrides the UDP source port in `from.port`. Removed from props before delivery. |

The `_replyaddr` / `_replyport` mechanism allows a responder behind NAT or on a different interface to specify the correct address for follow-up connections.

## Roon Core Service UUID

The well-known service UUID for Roon Core discovery:

```
00720724-5143-4a9b-abac-0e50cba674bb
```

A standard Roon discovery query contains:

```
query_service_id = "00720724-5143-4a9b-abac-0e50cba674bb"
```

## Socket Architecture

For each IPv4 network interface, two sockets are created:

### Receive Socket (per interface)
- Type: UDP4, `SO_REUSEADDR`
- Bound to: `0.0.0.0:9003` (the SOOD port)
- Joins multicast group `239.255.90.90` on the interface's IP
- Purpose: receives multicast and broadcast queries/responses

### Send Socket (per interface)
- Type: UDP4
- Bound to: `<interface_ip>:0` (ephemeral port)
- Options: `SO_BROADCAST` enabled, multicast TTL = 1
- Purpose: sends queries to both multicast and broadcast addresses
- Also receives responses (since queries carry the source port)

### Unicast Socket (global, one instance)
- Type: UDP4
- Bound to: `0.0.0.0:0` (ephemeral port)
- Options: `SO_BROADCAST` enabled, multicast TTL = 1
- Purpose: fallback sender when interface-specific sockets are unavailable; also sends to multicast address

## Sending a Query

When `query()` is called:

1. Ensure `_tid` is set (generate UUID v4 if missing).
2. Serialize the message into the binary wire format.
3. For each interface with an active send socket:
   - Send to `239.255.90.90:9003` (multicast)
   - Send to `<broadcast_address>:9003` (subnet broadcast, computed from interface IP and netmask)
4. If the unicast socket is active:
   - Send to `239.255.90.90:9003` (multicast)

## Interface Polling

Network interfaces are polled every **5 seconds** via `os.networkInterfaces()`.

Each poll cycle:
1. Enumerate all IPv4 addresses on all interfaces.
2. For new interfaces: create receive and send sockets.
3. For removed interfaces: destroy associated sockets and free resources.
4. If any interface was added or removed: emit a `network` event, which triggers an immediate discovery query.

An internal sequence counter (`_iface_seq`) tracks interface liveness. Interfaces not seen in the current poll cycle are considered removed.

## Discovery Query Scheduling

After `start()` is called:

1. Initialize sockets (first interface poll).
2. Send an immediate discovery query.
3. Start a periodic scan timer at **10 second** intervals.

The periodic scan uses adaptive frequency:
- First 6 ticks (0-5, covering the first ~60 seconds): query on every tick.
- After tick 5: query only every 6th tick (~60 second intervals).
- If already paired to a core: skip all periodic queries (the connection is already established).

## Lifecycle

```
start()
  ├── initsocket()          # first interface poll + socket creation
  │     └── (200ms delay)   # brief startup delay before callback
  ├── query(...)            # immediate discovery query
  └── setInterval(10s)      # periodic scan timer
       └── periodic_scan()

stop()
  ├── clearInterval()       # stop periodic scan
  ├── close recv_sock       # per interface
  ├── close send_sock       # per interface
  └── close unicast sock
```

## Complete Wire Example

### Query Packet

Querying for Roon Core:

```
Offset  Hex                          ASCII / Description
──────  ───────────────────────────  ───────────────────
0x00    53 4F 4F 44                  "SOOD" magic
0x04    02                           version 2
0x05    51                           "Q" (query)
0x06    10                           name_len = 16
0x07    71 75 65 72 79 5F 73 65      "query_se"
0x0F    72 76 69 63 65 5F 69 64      "rvice_id"
0x17    00 24                        value_len = 36
0x19    30 30 37 32 30 37 32 34      "00720724-5143-4a9b
        2D 35 31 34 33 2D 34 61       -abac-0e50cba674bb"
        39 62 2D 61 62 61 63 2D
        30 65 35 30 63 62 61 36
        37 34 62 62
0x3D    04                           name_len = 4
0x3E    5F 74 69 64                  "_tid"
0x42    00 24                        value_len = 36
0x44    <36 bytes UUID v4>           e.g. "a1b2c3d4-..."
```

### Response Packet

```
Offset  Hex                          Description
──────  ───────────────────────────  ───────────────────
0x00    53 4F 4F 44                  "SOOD" magic
0x04    02                           version 2
0x05    52                           "R" (response)
0x06    ...                          TLV: service_id = "00720724-..."
        ...                          TLV: unique_id = "<core-unique-id>"
        ...                          TLV: http_port = "9100"
        ...                          TLV: _tid = "<echoed-tid>"
```
