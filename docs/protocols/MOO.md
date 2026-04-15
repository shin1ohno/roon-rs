# MOO Protocol Specification

**Message-Oriented Object**

MOO is a text-header, binary-body protocol layered over WebSocket that provides request/response and subscription messaging between Roon extensions and Roon Core.

## Transport Layer

### WebSocket Connection

| Parameter    | Value                            |
|--------------|----------------------------------|
| URL          | `ws://<host>:<port>/api`         |
| Binary mode  | All frames sent as binary        |
| Masking      | Client-to-server frames are masked (per RFC 6455) |

The `host` and `port` are obtained from SOOD discovery (the `_replyaddr`/`from.ip` and `http_port` fields).

### Heartbeat

The client sends a WebSocket **ping** frame every **10 seconds**. The `is_alive` flag is set to `false` before each ping and set back to `true` when a **pong** is received.

If `is_alive` is still `false` when the next ping is due (i.e., no pong was received within 10 seconds), the connection is terminated immediately via `ws.terminate()`.

This means a connection is declared dead after approximately **10-20 seconds** of unresponsiveness.

## Message Wire Format

Each MOO message is a single WebSocket frame containing a text header section followed by an optional binary body.

```
MOO/1 <VERB> <name>\n
Header-Key: Header-Value\n
...\n
\n
[body bytes]
```

### First Line

```
MOO/1 <VERB> <name>
```

- **Protocol version**: always `MOO/1`
- **VERB**: one of `REQUEST`, `CONTINUE`, `COMPLETE`
- **name**: meaning depends on verb (see below)

The first line is terminated by a newline (`0x0A`).

### Headers

Each header is a line of the form:

```
Key: Value
```

Headers are terminated by a blank line (a newline immediately following the previous newline).

#### Standard Headers

| Header           | Required | Description                                                   |
|------------------|----------|---------------------------------------------------------------|
| `Request-Id`     | Always   | Integer identifying the request/response exchange             |
| `Content-Length`  | If body  | Length of the body in bytes                                   |
| `Content-Type`   | If body  | MIME type of the body                                         |
| `Logging`        | Optional | Set to `"quiet"` by Roon Core to suppress verbose logging     |

**Validation rules:**
- `Request-Id` is mandatory. Messages without it are discarded.
- If `Content-Type` is present, `Content-Length` must also be present (and vice versa for non-zero lengths).
- If neither `Content-Length` nor `Content-Type` is present, the message has no body.
- If `Content-Length` is present and > 0, `Content-Type` must be present.

Any additional headers (not listed above) are preserved in a generic headers map.

### Body

The body immediately follows the blank line that terminates the headers.

| Content-Type         | Handling                                         |
|----------------------|--------------------------------------------------|
| `application/json`   | UTF-8 encoded JSON. Parsed into a structured object. |
| Any other type       | Raw binary buffer. Passed through as-is.         |

If `Content-Length` is 0 or absent, there is no body.

## Verbs

### REQUEST

Initiates a new request from client to server (or server to client).

**First line format:**
```
MOO/1 REQUEST <service>/<method>
```

The `<name>` field is parsed as `<service>/<method>` where:
- `<service>`: the fully-qualified service name (e.g., `com.roonlabs.registry:1`)
- `<method>`: the method name within that service (e.g., `info`, `register`)

**Behavior:**
- The sender assigns a new `Request-Id` (monotonically increasing integer, starting at 0).
- The sender registers a callback for this `Request-Id` to handle responses.
- The receiver dispatches to the appropriate service handler based on `<service>`.

### CONTINUE

Sends an intermediate response for an in-flight request. The request remains open.

**First line format:**
```
MOO/1 CONTINUE <status>
```

- `<status>`: a status name (e.g., `Changed`, `Subscribed`)
- `Request-Id` must match an outstanding request.
- The request callback is invoked but the request is **not** removed from the pending table.

Used primarily for subscription updates (see Subscription Pattern below).

### COMPLETE

Sends the final response for a request. The request is closed.

**First line format:**
```
MOO/1 COMPLETE <status>
```

- `<status>`: a status name (e.g., `Success`, `InvalidRequest`, `Registered`)
- `Request-Id` must match an outstanding request.
- The request callback is invoked and the request **is** removed from the pending table.
- No further messages may reference this `Request-Id`.

## Request-Id Lifecycle

```
Client                                Server
  │                                     │
  │── REQUEST (id=0) ──────────────────>│
  │                                     │
  │<──────────── CONTINUE (id=0) ──────│   (optional, 0..N times)
  │                                     │
  │<──────────── COMPLETE (id=0) ──────│   (exactly once, closes id=0)
  │                                     │
  │── REQUEST (id=1) ──────────────────>│
  │       ...                           │
```

- Request-Ids are assigned by the sender and are scoped to the connection (each side maintains its own counter).
- A CONTINUE does not consume the Request-Id; only COMPLETE does.
- If a response arrives for an unknown Request-Id, the connection is closed.
- On connection close, all pending request callbacks are invoked with no arguments (signaling disconnection).

## Subscription Pattern

Subscriptions allow a server to push ongoing updates to a client. They are built on top of the CONTINUE/COMPLETE verbs.

### Subscribing

1. The client generates a `subscription_key` (monotonically increasing integer).
2. The client sends a REQUEST:
   ```
   MOO/1 REQUEST <service>/subscribe_<name>
   Request-Id: <N>
   Content-Length: <len>
   Content-Type: application/json

   {"subscription_key": <key>, ...additional_args}
   ```
3. The server stores the subscription keyed by `(moo_id, subscription_key)`.
4. The server sends an initial CONTINUE with the current state:
   ```
   MOO/1 CONTINUE Subscribed
   Request-Id: <N>
   Content-Length: <len>
   Content-Type: application/json

   {...initial_state}
   ```
5. Subsequently, the server sends CONTINUE messages whenever the state changes:
   ```
   MOO/1 CONTINUE Changed
   Request-Id: <N>
   Content-Length: <len>
   Content-Type: application/json

   {...updated_state}
   ```

The request remains open (no COMPLETE sent) for the lifetime of the subscription.

### Unsubscribing

The client sends a separate REQUEST:
```
MOO/1 REQUEST <service>/unsubscribe_<name>
Request-Id: <M>
Content-Length: <len>
Content-Type: application/json

{"subscription_key": <key>}
```

The server responds:
```
MOO/1 COMPLETE Unsubscribed
Request-Id: <M>
```

And also sends COMPLETE on the original subscription request, closing it.

### Connection Loss

When a connection closes, the server removes all subscriptions associated with that connection's `moo_id`.

## Registry Protocol

Service name: `com.roonlabs.registry:1`

The registry protocol is the first exchange on every new MOO connection. It authenticates the extension and establishes which services are available.

### Step 1: Info Request

The client sends:
```
MOO/1 REQUEST com.roonlabs.registry:1/info
Request-Id: 0
```

The server responds with core information:
```
MOO/1 COMPLETE Success
Request-Id: 0
Content-Length: <len>
Content-Type: application/json

{
  "core_id": "<unique-core-id>",
  "display_name": "...",
  "display_version": "..."
}
```

The client uses `core_id` to look up a previously persisted authentication token.

### Step 2: Register

The client sends its extension registration info:
```
MOO/1 REQUEST com.roonlabs.registry:1/register
Request-Id: 1
Content-Length: <len>
Content-Type: application/json

{
  "extension_id": "com.example.my_extension",
  "display_name": "My Extension",
  "display_version": "1.0.0",
  "publisher": "Example",
  "email": "dev@example.com",
  "website": "https://example.com",
  "token": "<previously-saved-token-or-omitted>",
  "required_services": ["com.roonlabs.transport:2"],
  "optional_services": ["com.roonlabs.browse:1"],
  "provided_services": [
    "com.roonlabs.pairing:1",
    "com.roonlabs.ping:1"
  ]
}
```

The server responds:
```
MOO/1 CONTINUE Registered
Request-Id: 1
Content-Length: <len>
Content-Type: application/json

{
  "core_id": "<core-id>",
  "display_name": "My Roon Core",
  "display_version": "2.0",
  "token": "<auth-token>",
  "provided_services": ["com.roonlabs.transport:2", "com.roonlabs.browse:1"],
  "http_port": 9100
}
```

**Important:** The response verb is `CONTINUE`, not `COMPLETE`. The registration request remains open for the lifetime of the connection. If the connection is lost, the pending callback is invoked with no arguments, signaling disconnection.

### Token Persistence

- The `token` in the `Registered` response must be persisted, keyed by `core_id`.
- On subsequent connections to the same core, the persisted token is included in the `register` request.
- The token authorizes the extension without requiring the user to re-approve it in Roon's UI.
- Tokens are stored in a `config.json` file (Node.js) or `localStorage` (browser) under the path `roonstate.tokens.<core_id>`.

### Alternative: One-Time Token Registration

An alternative registration method exists for pre-authorized connections:

```
MOO/1 REQUEST com.roonlabs.registry:1/register_one_time_token
Request-Id: 0
Content-Length: <len>
Content-Type: application/json

{
  ...extension_reginfo,
  "token": "<one-time-token>"
}
```

This skips the `info` step and uses a pre-shared token. The response is the same `Registered` message.

## Pairing Protocol

Service name: `com.roonlabs.pairing:1`

Pairing restricts the extension to communicate with exactly one Roon Core at a time. It is a service **provided by the extension** (not the core).

### Methods

#### `get_pairing`

Returns the current pairing state.

Request:
```
MOO/1 REQUEST com.roonlabs.pairing:1/get_pairing
Request-Id: <N>
```

Response:
```
MOO/1 COMPLETE Success
Request-Id: <N>
Content-Length: <len>
Content-Type: application/json

{"paired_core_id": "<core-id-or-undefined>"}
```

#### `pair`

Pairs with the requesting core, unpairing from any previously paired core.

Request:
```
MOO/1 REQUEST com.roonlabs.pairing:1/pair
Request-Id: <N>
```

Side effects:
1. If already paired to a different core, the old core is "lost" (unpaired callback fires).
2. The new core becomes the paired core.
3. `paired_core_id` is persisted to state.
4. All `subscribe_pairing` subscribers receive a `Changed` notification.

#### `subscribe_pairing` / `unsubscribe_pairing`

Standard subscription pattern. Initial state and updates carry:

```json
{"paired_core_id": "<core-id>"}
```

### Pairing Lifecycle

```
Extension                        Core A                Core B
  │                                │                     │
  │<── register (core A) ─────────│                     │
  │    (first core found)          │                     │
  │    auto-pair with Core A       │                     │
  │    persist paired_core_id      │                     │
  │                                │                     │
  │    core_paired(A) fires        │                     │
  │                                │                     │
  │<── pair request ───────────────────────────────────── │
  │    unpair Core A               │                     │
  │    core_unpaired(A) fires      │                     │
  │    pair with Core B            │                     │
  │    core_paired(B) fires        │                     │
  │    notify subscribers          │                     │
```

When paired, periodic SOOD discovery queries are suppressed to reduce network traffic.

## Ping Protocol

Service name: `com.roonlabs.ping:1`

A minimal service provided by every extension for health checking.

### Methods

#### `ping`

Request:
```
MOO/1 REQUEST com.roonlabs.ping:1/ping
Request-Id: <N>
```

Response:
```
MOO/1 COMPLETE Success
Request-Id: <N>
```

No body in either direction.

## Error Handling

### Unknown Service

When a REQUEST arrives for an unregistered service:

```
MOO/1 COMPLETE InvalidRequest
Request-Id: <N>
Content-Length: <len>
Content-Type: application/json

{"error": "unknown service: <service-name>"}
```

### Unknown Method

When a REQUEST arrives for a registered service but unknown method:

```
MOO/1 COMPLETE InvalidRequest
Request-Id: <N>
Content-Length: <len>
Content-Type: application/json

{"error": "unknown request name (<service-name>) : <method-name>"}
```

### Unknown Request-Id in Response

If a CONTINUE or COMPLETE arrives with a `Request-Id` that is not in the pending requests table, the connection is closed immediately. This prevents state desynchronization.

### Connection Loss

On WebSocket close:
1. All registered service handlers are notified with a `null` request (signaling the connection's moo_id is gone), allowing cleanup of subscriptions.
2. All pending request callbacks are invoked with no arguments.
3. The pending requests table is cleared.

## Complete Message Examples

### Simple Request/Response (Ping)

Client sends:
```
MOO/1 REQUEST com.roonlabs.ping:1/ping
Request-Id: 5

```

Server responds:
```
MOO/1 COMPLETE Success
Request-Id: 5

```

### Request with JSON Body

Client sends:
```
MOO/1 REQUEST com.roonlabs.transport:2/seek
Request-Id: 12
Content-Length: 52
Content-Type: application/json

{"zone_id":"1601234567890","how":"absolute","seconds":30}
```

Server responds:
```
MOO/1 COMPLETE Success
Request-Id: 12

```

### Subscription Flow

Client subscribes:
```
MOO/1 REQUEST com.roonlabs.transport:2/subscribe_zones
Request-Id: 3
Content-Length: 21
Content-Type: application/json

{"subscription_key":0}
```

Server sends initial state:
```
MOO/1 CONTINUE Subscribed
Request-Id: 3
Content-Length: 1842
Content-Type: application/json

{"zones":[{"zone_id":"...","display_name":"Living Room",...}]}
```

Server pushes update:
```
MOO/1 CONTINUE Changed
Request-Id: 3
Content-Length: 327
Content-Type: application/json

{"zones_changed":[{"zone_id":"...","state":"playing",...}]}
```

Client unsubscribes:
```
MOO/1 REQUEST com.roonlabs.transport:2/unsubscribe_zones
Request-Id: 14
Content-Length: 21
Content-Type: application/json

{"subscription_key":0}
```

Server responds:
```
MOO/1 COMPLETE Unsubscribed
Request-Id: 14

```

### Full Registration Handshake

```
--> MOO/1 REQUEST com.roonlabs.registry:1/info
    Request-Id: 0

<-- MOO/1 COMPLETE Success
    Request-Id: 0
    Content-Length: 89
    Content-Type: application/json

    {"core_id":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","display_name":"My Core","display_version":"2.0"}

--> MOO/1 REQUEST com.roonlabs.registry:1/register
    Request-Id: 1
    Content-Length: 312
    Content-Type: application/json

    {"extension_id":"com.example.ext","display_name":"My Ext","display_version":"1.0.0","publisher":"Me","email":"me@example.com","token":"prev-token-if-any","required_services":[],"optional_services":[],"provided_services":["com.roonlabs.pairing:1","com.roonlabs.ping:1"]}

<-- MOO/1 CONTINUE Registered
    Request-Id: 1
    Content-Length: 198
    Content-Type: application/json

    {"core_id":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx","display_name":"My Core","display_version":"2.0","token":"new-auth-token","provided_services":["com.roonlabs.transport:2"],"http_port":9100}
```

Note: The `register` request receives `CONTINUE Registered` (not `COMPLETE`), keeping the request open for the connection lifetime.
