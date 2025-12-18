# Sync Protocol Specification (Normative)

This document defines the **EntiDB synchronization protocol** for logical replication between EntiDB instances.

This document is **normative**. All sync implementations **MUST** conform to this specification.

---

## 1. Overview

### 1.1 Purpose

The sync protocol enables:

* **Offline-first operation**: Clients work independently and sync when connectivity is available.
* **Pull-then-push replication**: Clients first fetch server changes, then push local changes.
* **Conflict detection**: Server detects when client and server have diverged.
* **Cursor-based pagination**: Efficient incremental sync via monotonic cursors.

### 1.2 Design Principles

* **CBOR-native**: All messages are encoded as canonical CBOR.
* **Server authority**: The server is the authoritative source of truth.
* **Explicit configuration**: No implicit discovery or magic behavior.
* **EntiDB everywhere**: Both client and server use the same EntiDB engine.
* **Transport agnostic**: Protocol operates over HTTPS; WebSocket is optional for real-time updates.

### 1.3 Non-Goals

* Real-time collaboration (eventual consistency only)
* Peer-to-peer synchronization
* Automatic conflict resolution (policy is configurable)

---

## 2. Protocol Versioning

### 2.1 Version Format

```
Version := (major: u16, minor: u16)
```

### 2.2 Compatibility Rules

* **Major version mismatch**: Handshake **MUST** fail.
* **Minor version mismatch**: Higher minor **MAY** be accepted; unknown fields **MUST** be ignored.

### 2.3 Current Version

```
PROTOCOL_VERSION_MAJOR = 1
PROTOCOL_VERSION_MINOR = 0
MIN_SUPPORTED_MAJOR = 1
MIN_SUPPORTED_MINOR = 0
```

---

## 3. Core Data Types

### 3.1 SyncOperation

A `SyncOperation` represents a single committed entity mutation for replication.

```
SyncOperation {
    op_id:          i64,        // Monotonic operation ID (local to originating device)
    db_id:          String,     // Database identifier (globally unique)
    device_id:      String,     // Device identifier (stable per device)
    collection:     String,     // Collection name
    entity_id:      String,     // Entity identifier
    op_type:        OpType,     // Operation type
    entity_version: i64,        // Entity version (monotonic per entity)
    entity_cbor:    Option<Vec<u8>>, // Entity payload as canonical CBOR (None for deletes)
    timestamp_ms:   i64,        // Client timestamp (informational only)
}

OpType := "upsert" | "delete"
```

**Encoding Rules:**

* `entity_cbor` **MUST** be canonical CBOR bytes.
* `entity_cbor` **MUST** be `None` for delete operations.
* `timestamp_ms` is informational and **MUST NOT** be used for ordering.

### 3.2 Conflict

A `Conflict` represents a detected divergence between client and server state.

```
Conflict {
    collection:     String,
    entity_id:      String,
    client_op:      SyncOperation,
    server_state:   ServerState,
}

ServerState {
    entity_version: i64,
    entity_cbor:    Option<Vec<u8>>,  // None if entity deleted on server
}
```

### 3.3 SyncCursor

A cursor tracks synchronization progress for resumable sync.

```
SyncCursor {
    last_op_id:     i64,        // Last synchronized operation ID
    server_cursor:  i64,        // Server cursor position
    last_sync_at:   i64,        // Timestamp of last successful sync (ms since epoch)
}
```

### 3.4 ClientInfo

Client metadata provided during handshake.

```
ClientInfo {
    platform:       String,     // Platform identifier (e.g., "windows", "android", "web")
    app_version:    String,     // Application version string
}
```

### 3.5 ServerCapabilities

Server capabilities returned during handshake.

```
ServerCapabilities {
    pull:   bool,   // Server supports pull operations
    push:   bool,   // Server supports push operations
    sse:    bool,   // Server supports Server-Sent Events
}
```

---

## 4. Protocol Messages

### 4.1 Handshake

Establishes connection and exchanges capabilities.

**Request:**

```
POST /v1/handshake
Content-Type: application/cbor

HandshakeRequest {
    db_id:          String,
    device_id:      String,
    client_info:    ClientInfo,
    protocol_version: (u16, u16),
}
```

**Response:**

```
HandshakeResponse {
    server_cursor:  i64,
    capabilities:   ServerCapabilities,
}
```

**Errors:**

| Code | Meaning |
|------|---------|
| `version_mismatch` | Protocol version incompatible |
| `database_not_found` | Database ID not recognized |
| `authentication_failed` | Auth token invalid |

### 4.2 Pull

Fetches operations from server since a given cursor.

**Request:**

```
POST /v1/pull
Content-Type: application/cbor

PullRequest {
    db_id:          String,
    since_cursor:   i64,        // 0 for initial sync
    limit:          i64,        // Max operations to return (default: 100)
    collections:    Option<Vec<String>>,  // Optional collection filter
}
```

**Response:**

```
PullResponse {
    ops:            Vec<SyncOperation>,
    next_cursor:    i64,        // Use as since_cursor in next request
    has_more:       bool,       // If true, make another pull request
}
```

**Semantics:**

* `since_cursor` is **exclusive**: operations with `op_id > since_cursor` are returned.
* If `collections` is provided, only operations for those collections are returned.
* `next_cursor` equals the highest `op_id` in `ops`, or `since_cursor` if empty.

### 4.3 Push

Submits local operations to the server.

**Request:**

```
POST /v1/push
Content-Type: application/cbor

PushRequest {
    db_id:          String,
    device_id:      String,
    ops:            Vec<SyncOperation>,
}
```

**Response:**

```
PushResponse {
    acknowledged_up_to_op_id: i64,
    conflicts:      Vec<Conflict>,
}
```

**Semantics:**

* Operations are applied atomically on the server.
* `acknowledged_up_to_op_id` is the highest `op_id` successfully accepted.
* `conflicts` contains operations that conflicted with server state.
* Conflicting operations are **not** applied; client must resolve and retry.

---

## 5. Error Handling

### 5.1 ErrorResponse

All endpoints may return an error response.

```
ErrorResponse {
    code:       SyncErrorCode,
    message:    String,
    details:    Option<Map<String, Value>>,
}
```

### 5.2 Error Codes

```rust
enum SyncErrorCode {
    Unknown             = 0,
    InvalidRequest      = 1,
    AuthenticationFailed = 2,
    AuthorizationFailed = 3,
    DatabaseNotFound    = 4,
    VersionMismatch     = 5,
    Conflict            = 6,
    RateLimitExceeded   = 7,
    InternalError       = 8,
    ServiceUnavailable  = 9,
    Timeout             = 10,
    InvalidCursor       = 11,
}
```

---

## 6. Sync Engine State Machine

### 6.1 States

```
idle → connecting → pulling → pushing → synced → (back to idle)
                 ↘          ↘         ↘
                        error (can retry → connecting)
```

| State | Description |
|-------|-------------|
| `idle` | No sync in progress, waiting for trigger |
| `connecting` | Establishing connection, performing handshake |
| `pulling` | Fetching server operations since last cursor |
| `pushing` | Sending local operations to server |
| `synced` | Cycle complete, will return to idle |
| `error` | Recoverable failure, can retry with backoff |

### 6.2 Sync Cycle

A complete sync cycle follows this sequence:

1. **Handshake**: Exchange capabilities and get server cursor.
2. **Pull**: Fetch all server operations since client's cursor (paginated).
3. **Apply**: Apply pulled operations to local EntiDB transactionally.
4. **Push**: Send local pending operations to server.
5. **Handle Conflicts**: Process any conflicts returned by server.
6. **Complete**: Update cursors and return to idle.

---

## 7. Conflict Resolution

### 7.1 Conflict Detection

A conflict occurs when:

* Client pushes an operation for `(collection, entity_id)`.
* Server has a different `entity_version` than client expected.

### 7.2 Resolution Policies

Resolution is a **policy choice**, not inherent to the protocol.

| Policy | Description |
|--------|-------------|
| `server_wins` | Discard client changes, accept server state |
| `client_wins` | Retry push with force (if server allows) |
| `last_write_wins` | Compare timestamps, accept latest |
| `manual` | Surface to application for user resolution |

### 7.3 Default Policy

The default policy is **server wins**. The server is authoritative.

---

## 8. Transport Requirements

### 8.1 HTTPS

* All sync traffic **MUST** use HTTPS in production.
* Content-Type **MUST** be `application/cbor`.
* Authentication tokens **MUST** be passed via `Authorization` header.

### 8.2 Request Format

```
POST /{endpoint}
Authorization: Bearer {token}
Content-Type: application/cbor
Content-Length: {length}

{CBOR-encoded request body}
```

### 8.3 Response Format

```
HTTP/1.1 200 OK
Content-Type: application/cbor
Content-Length: {length}

{CBOR-encoded response body}
```

### 8.4 HTTP Status Codes

| Status | Meaning |
|--------|---------|
| 200 | Success |
| 400 | Invalid request |
| 401 | Authentication required |
| 403 | Authorization failed |
| 404 | Resource not found |
| 409 | Conflict |
| 429 | Rate limit exceeded |
| 500 | Internal server error |
| 503 | Service unavailable |

---

## 9. Real-Time Updates (Optional)

### 9.1 Server-Sent Events (SSE)

For real-time updates, clients **MAY** subscribe to an SSE stream.

```
GET /v1/stream
Authorization: Bearer {token}

event: operation
data: {JSON-encoded SyncOperation}

event: operation
data: {JSON-encoded SyncOperation}
```

### 9.2 WebSocket (Alternative)

WebSocket transport provides bidirectional real-time sync.

**Message Types:**

| Type | Direction | Purpose |
|------|-----------|---------|
| `subscribe` | C→S | Subscribe to updates |
| `subscribed` | S→C | Subscription confirmed |
| `operations` | S→C | New operations available |
| `pull` | C→S | Pull request |
| `pull_response` | S→C | Pull response |
| `push` | C→S | Push request |
| `push_response` | S→C | Push response |
| `ping` / `pong` | Both | Keepalive |
| `error` | S→C | Error notification |

---

## 10. CBOR Encoding

### 10.1 Canonical CBOR Rules

All protocol messages **MUST** use canonical CBOR:

* Maps **MUST** be sorted by key (bytewise).
* Integers **MUST** use shortest encoding.
* Strings **MUST** be UTF-8.
* Indefinite-length items: **FORBIDDEN**.
* NaN values: **FORBIDDEN**.

### 10.2 SyncOperation CBOR Structure

```cbor-diagnostic
{
  "opId": 42,
  "dbId": "production-db",
  "deviceId": "device-abc-123",
  "collection": "users",
  "entityId": "user-001",
  "opType": "upsert",
  "entityVersion": 3,
  "entityCbor": h'A26469...',  ; Raw CBOR bytes
  "timestampMs": 1702569600000
}
```

**Note:** Map keys are strings for readability but **MUST** be sorted bytewise in the encoded form.

---

## 11. Server Requirements

### 11.1 Server Role

The sync server:

* Hosts an authoritative EntiDB instance (same engine as clients).
* Maintains a server-side operation log (oplog).
* Assigns globally-ordered cursors to operations.
* Enforces authentication and authorization.
* Detects conflicts and applies resolution policy.

### 11.2 Server Oplog

The server maintains an append-only log of all committed operations:

```
ServerOplogEntry {
    server_op_id:   i64,        // Server-assigned, globally monotonic
    client_op_id:   i64,        // Original client op_id
    device_id:      String,
    collection:     String,
    entity_id:      String,
    op_type:        OpType,
    entity_version: i64,
    entity_cbor:    Option<Vec<u8>>,
    received_at:    i64,        // Server timestamp
}
```

### 11.3 Cursor Assignment

* Server cursors are monotonically increasing integers.
* Each accepted operation is assigned the next cursor value.
* Cursors define global visibility order.

---

## 12. Client Requirements

### 12.1 Local State

Clients **MUST** persist:

* `device_id`: Stable identifier for this device.
* `db_id`: Database identifier.
* `server_cursor`: Last pulled cursor position.
* `local_cursor`: Last pushed operation ID.

### 12.2 Offline Queue

Clients **MUST** maintain an offline queue of pending operations:

* Operations are queued when local changes are committed.
* Queue is drained during push phase.
* Acknowledged operations are removed from queue.

### 12.3 Idempotency

* Applying the same `SyncOperation` multiple times **MUST NOT** change final state.
* Operations are keyed by `(collection, entity_id, entity_version)`.

---

## 13. Invariants

### 13.1 Protocol Invariants

* Only **committed** operations appear in sync streams.
* Sync **MUST NOT** corrupt local EntiDB state.
* Cursor order **MUST** match commit order.
* Entity payloads **MUST** remain canonical CBOR throughout.

### 13.2 Sync Invariants

* Pull-then-push: Client always pulls before pushing.
* Server authority: Server state wins by default.
* Offline-first: Full local functionality without network.
* Graceful degradation: Sync failures never corrupt local state.

---

## 14. Security Considerations

### 14.1 Authentication

* Clients **MUST** authenticate via Bearer token.
* Tokens **MUST** be obtained through a separate auth flow.
* Tokens **SHOULD** have limited lifetime.

### 14.2 Authorization

* Server **MUST** verify client access to requested database.
* Server **MAY** enforce per-collection access control.
* Server **MUST** verify device_id matches authenticated identity.

### 14.3 Transport Security

* TLS 1.2+ **MUST** be used for all sync traffic.
* Certificate validation **MUST NOT** be disabled in production.

---

## 15. Rust Type Definitions

```rust
/// Operation type for sync operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Upsert,
    Delete,
}

/// A single synchronization operation.
#[derive(Debug, Clone)]
pub struct SyncOperation {
    pub op_id: i64,
    pub db_id: String,
    pub device_id: String,
    pub collection: String,
    pub entity_id: String,
    pub op_type: OpType,
    pub entity_version: i64,
    pub entity_cbor: Option<Vec<u8>>,
    pub timestamp_ms: i64,
}

/// Conflict between client and server state.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub collection: String,
    pub entity_id: String,
    pub client_op: SyncOperation,
    pub server_state: ServerState,
}

/// Server state for conflict resolution.
#[derive(Debug, Clone)]
pub struct ServerState {
    pub entity_version: i64,
    pub entity_cbor: Option<Vec<u8>>,
}

/// Sync cursor for progress tracking.
#[derive(Debug, Clone, Default)]
pub struct SyncCursor {
    pub last_op_id: i64,
    pub server_cursor: i64,
    pub last_sync_at: i64,
}

/// Sync engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Connecting,
    Pulling,
    Pushing,
    Synced,
    Error,
}

/// Result of a sync cycle.
#[derive(Debug)]
pub struct SyncResult {
    pub state: SyncState,
    pub pulled_count: usize,
    pub pushed_count: usize,
    pub conflicts: Vec<Conflict>,
    pub error: Option<SyncError>,
    pub server_cursor: i64,
}

/// Sync error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SyncErrorCode {
    Unknown = 0,
    InvalidRequest = 1,
    AuthenticationFailed = 2,
    AuthorizationFailed = 3,
    DatabaseNotFound = 4,
    VersionMismatch = 5,
    Conflict = 6,
    RateLimitExceeded = 7,
    InternalError = 8,
    ServiceUnavailable = 9,
    Timeout = 10,
    InvalidCursor = 11,
}
```

---

## 16. Sequence Diagrams

### 16.1 Normal Sync (No Conflict)

```
Client (EntiDB)         Server (EntiDB)
     │                       │
     │── handshake (CBOR) ──►│
     │◄── cursor (CBOR) ─────│
     │                       │
     │── pull (CBOR) ───────►│
     │◄── ops (CBOR) ────────│
     │   apply ops locally   │
     │                       │
     │── push (CBOR) ───────►│
     │                       │  server tx commit
     │                       │  append server oplog
     │◄── ack (CBOR) ────────│
     │                       │
```

### 16.2 Sync with Conflict

```
Client (EntiDB)         Server (EntiDB)
     │                       │
     │── handshake ─────────►│
     │◄── cursor ────────────│
     │                       │
     │── pull ──────────────►│
     │◄── ops ───────────────│
     │   apply ops           │
     │                       │
     │── push ──────────────►│
     │                       │  detect conflict
     │◄── conflicts ─────────│
     │                       │
     │   resolve conflict    │
     │   (policy decision)   │
     │                       │
     │── push (resolved) ───►│
     │◄── ack ───────────────│
     │                       │
```

---

## 17. Test Vectors

Test vectors for protocol messages are provided in `docs/test_vectors/sync_protocol/`.

Each test vector includes:

* Logical structure (JSON-like)
* Canonical CBOR hex encoding
* Expected decoded values

---

## 18. Implementation Checklist

### 18.1 Protocol Crate (`entidb_sync_protocol`)

- [ ] `SyncOperation` with CBOR encode/decode
- [ ] `Conflict` with CBOR encode/decode
- [ ] `SyncCursor` with JSON serialization
- [ ] `HandshakeRequest` / `HandshakeResponse`
- [ ] `PullRequest` / `PullResponse`
- [ ] `PushRequest` / `PushResponse`
- [ ] `ErrorResponse` with error codes
- [ ] Protocol version constants
- [ ] Test vectors passing

### 18.2 Sync Engine (`entidb_sync_engine`)

- [ ] State machine implementation
- [ ] Cursor management
- [ ] Conflict detection
- [ ] Retry with exponential backoff

### 18.3 Sync Server (`entidb_sync_server`)

- [ ] HTTP endpoints (handshake, pull, push)
- [ ] Server oplog persistence
- [ ] Conflict detection
- [ ] Authentication middleware
- [ ] SSE/WebSocket (optional)

---

## 19. References

* [architecture.md](architecture.md) — System architecture
* [cbor_canonical.md](cbor_cannonical.md) — CBOR encoding rules
* [transactions.md](transactions.md) — Transaction semantics
* [invariants.md](invariants.md) — Global invariants
* [AGENTS.md](../AGENTS.md) — Agent instructions
