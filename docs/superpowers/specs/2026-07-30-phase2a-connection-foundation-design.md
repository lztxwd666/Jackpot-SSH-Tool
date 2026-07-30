# Phase 2a Connection Foundation Design

> Stage 2a of the roadmap: SSH connection with authentication and host key verification.

## Scope

Implement SSH connection capability — TCP connect, handshake, authenticate, verify host keys, clean disconnect. No sessions, channels, keepalive, or reconnection (these are 2b).

## Crate Changes

### core-common — New SSH Types

```rust
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub timeout_secs: u64,
}

pub enum AuthMethod {
    Password(String),
    PrivateKey { path: PathBuf, passphrase: Option<String> },
    Agent,
}

pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}
```

### core-event — New Event Variants

Add to `CoreEvent`:
```rust
pub enum CoreEvent {
    Application(ApplicationEvent),
    System(SystemEvent),
    Connection(ConnectionEvent),      // new
    HostKey(HostKeyEvent),            // new
    Credential(CredentialEvent),      // new
}

pub enum ConnectionEvent {
    Connecting { host: String, port: u16 },
    TcpConnected,
    HandshakeStarted,
    HostKeyVerifying,
    Authenticated,
    Ready,
    Disconnected,
    Failed { reason: String },
}

pub enum HostKeyEvent {
    Unknown { host: String, fingerprint: String },
    Changed { host: String, old_fingerprint: String, new_fingerprint: String },
    Accepted,
    Rejected,
}

pub enum CredentialEvent {
    Loaded,
    NotFound(String),
    AccessDenied(String),
}
```

### core-runtime — SSH Connection Module

New files under `crates/core-runtime/src/ssh/`:
- `connection.rs` — `SshConnection` wrapping `ssh2::Session`, manages raw lifecycle
- `auth.rs` — `authenticate(session, config)` with password and key support
- `hostkey.rs` — `verify_host_key(session, config, known_hosts)` logic

New files at crate root:
- `connection_service.rs` — `ConnectionService` trait + `SshConnectionService` impl
- `knownhosts.rs` — `KnownHostsProvider` trait
- `credential.rs` — `CredentialProvider` trait + `ConfigCredentialProvider`

New dependency: `ssh2 = "0.9"`

### core-storage — HostKey Persistence

- Migration V2: add `known_hosts` table
- `KnownHostsProvider` impl backed by SQLite
- Pub fn: `store_host_key`, `find_host_key`, `list_host_keys`

## Data Flow

```
User IPC command → ConnectionService.connect(config)
  → SshConnection:
      1. TCP connect → ConnectionEvent::Connecting → TcpConnected
      2. SSH handshake → ConnectionEvent::HandshakeStarted
      3. HostKey verification → KnownHostsProvider → HostKeyEvent::Unknown/Accepted
      4. Authentication → CredentialProvider → ConnectionEvent::Authenticated
      5. Ready → ConnectionEvent::Ready
      6. Disconnect → ConnectionEvent::Disconnected
```

## Architecture Rules

- Core crates NEVER depend on desktop/Tauri
- Events are immutable, describe what happened
- Service → Domain → Infrastructure (top-down)
- All SSH I/O is async-compatible (runs in spawn_blocking)

## Testing Strategy

- Unit tests: auth logic, hostkey parsing, KnownHosts CRUD
- Integration: connect to local SSH server for full roundtrip
- Mock: KnownHostsProvider, CredentialProvider for service tests
