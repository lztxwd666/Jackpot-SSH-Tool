# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Jackpot SSH Tool — desktop SSH client. Rust workspace (Tauri v2 + Vue 3). Currently functional: host CRUD in SQLite, SSH connection with password/key auth, keepalive, exponential-backoff reconnect, and xterm.js terminal in the UI.

## Build, Test & Verification

`rust-toolchain.toml` pins Rust to 1.95.0. System `rustc` (apt) is 1.75 — too old. Ensure rustup binaries are first in `PATH`:

```bash
PATH="$HOME/.cargo/bin:$PATH"
```

```bash
# Workspace-wide
cargo build
cargo test
cargo clippy --all-targets -- -D warnings

# Per-crate tests
cargo test -p core-event
cargo test -p core-storage

# Architecture invariant: core crates must NOT depend on Tauri
cargo tree -p core-common --invert | grep tauri    # must be empty

# Tauri dev server (starts Vue + Rust — Windows only; WSL needs GTK libs)
npm run dev

# Frontend (inside crates/desktop/ui/ — NOT at repo root)
cd crates/desktop/ui && npm install
cd crates/desktop/ui && npm run build
cd crates/desktop/ui && npm run type-check

# Low memory: limit parallel rustc jobs
CARGO_BUILD_JOBS=1 cargo build
```

`.cargo/config.toml` redirects WSL builds to `target-wsl/` to avoid conflicts with Windows `target/`.

Tests in `core-storage` create temp directories with real SQLite files. They clean up on success but may leave artifacts on failure in `$TMPDIR`.

**Before reporting any implementation as complete, you MUST run all three** (`cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`). If any fails, fix before reporting. Never claim "done" without verified build + test + clippy.

## Architecture

Layered, strict top-down. Core crates must never import Tauri.

```
desktop ──→ core-runtime ──→ core-event
                │                 │
                └──→ core-common ←┘
                │
                └──→ core-storage
```

Full spec at `docs/core_idea_zh/` (Chinese) and `docs/core_idea_en/` (English). Read `01_Architecture` and `09_Engineering_Guidelines` before significant changes.

### Crate Map

| Crate          | Dir                    | Purpose                                                                                | Key Types                                                                                                                                                                                                                                                   |
| -------------- | ---------------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core-common`  | `crates/core-common/`  | Shared types, error enum, ID newtypes (UUID via `define_id!` macro), trait definitions | `CoreError`, `Host`, `HostId`/`SessionId`/`ChannelId`, `ConnectionConfig`, `AuthMethod`, `SessionState`, `ChannelType`, `ChannelState`, `PtySize`, `ReconnectPolicy`, `HostKeyInfo`, `Config`, `HostRepository`, `KnownHostsProvider`, `CredentialProvider` |
| `core-event`   | `crates/core-event/`   | Immutable event definitions, broadcast channel dispatcher                              | `CoreEvent` (tag-based JSON via `#[serde(tag = "type")]`), `ChannelDispatcher`, `ApplicationEvent`, `SystemEvent`, `ConnectionEvent`, `HostKeyEvent`, `CredentialEvent`, `SessionEvent`, `ChannelEvent`, `HostEvent`                                        |
| `core-runtime` | `crates/core-runtime/` | Application lifecycle, session/channel management, SSH engine, keepalive and reconnect | `CoreRuntime`, `Session`, `Channel`, `SshConnection`, `SshConnectionService`, `ConnectionService` trait, `spawn_keepalive()`, `spawn_reconnect()`                                                                                                           |
| `core-storage` | `crates/core-storage/` | SQLite + WAL, schema migrations, `Database::execute()` is the sole DB access gate      | `Database`, `SqliteHostRepository`, `SqliteKnownHosts`                                                                                                                                                                                                      |
| `desktop`      | `crates/desktop/`      | Tauri v2 app, IPC bridge, Vue 3 + xterm.js terminal UI                                 | `AppState`, Tauri commands                                                                                                                                                                                                                                  |

### SSH Connection Pipeline

```
SshConnection::connect()
  ├─ TCP connect (TcpStream::connect_timeout)
  ├─ ssh2::Session::handshake()
  ├─ host_key check → HostKeyEvent (Unknown/Accepted/Rejected)
  ├─ authenticate() → password | pubkey_file | Agent(未实现) 认证
  └─ Ready event
```

`SshConnection` holds `Option<ssh2::Session>`; disconnects on `Drop`. `Session` (higher-level) wraps `SshConnection` with state machine (`Created → Connecting → Connected → Disconnected → Closed`), owns a `Vec<Arc<Channel>>`, and supports reconnect.

`Channel` wraps either `ssh2::Channel` (Shell with PTY) or `ssh2::Sftp`. All blocking I/O runs in `tokio::task::spawn_blocking`. `Channel::start_read_loop()` spawns a background task that reads and emits `DataReceived` events.

### Event Flow

```
CoreRuntime → ChannelDispatcher (broadcast) → desktop/lib.rs event loop → tauri.emit("core-event") → Vue listen("core-event")
```

Event JSON is tag-based: `{"type": "Channel", "payload": {"kind": "DataReceived", "detail": {"channel_id": "...", "data": [...]}}}`. The Vue `Terminal` component filters by `channel_id` and writes bytes to xterm. User keystrokes go back via `terminal_send_input` IPC command.

Add new event variants to `CoreEvent` enum in `core-event/src/event.rs`. Events are immutable, describe things that already happened, and carry minimal payloads.

### Tauri IPC Commands

All commands in `desktop/src/commands.rs`. Shared state: `AppState { runtime, channels }` (via `tauri::State`).

| Command                                                     | Purpose                                                   |
| ----------------------------------------------------------- | --------------------------------------------------------- |
| `get_app_status`                                            | Returns "running" if CoreRuntime is initialized           |
| `ping`                                                      | Health check, returns "pong"                              |
| `list_hosts` / `save_host` / `delete_host` / `search_hosts` | Host CRUD                                                 |
| `create_session`                                            | Creates Session, returns session_id                       |
| `connect_session`                                           | Calls `Session::connect()` in spawn_blocking              |
| `open_shell`                                                | Opens Shell channel, returns channel_id, starts read loop |
| `terminal_send_input`                                       | Writes bytes to channel                                   |
| `terminal_resize`                                           | Resizes PTY (cols, rows)                                  |
| `terminal_close`                                            | Closes session, removes channels                          |

### Trait Abstractions

Core traits defined in `core-common`, implemented in `core-storage`:

- `HostRepository` → `SqliteHostRepository` (hosts table)
- `KnownHostsProvider` → `SqliteKnownHosts` (known_hosts table)
- `CredentialProvider` → `ConfigCredentialProvider` (stub, delegates to ConnectionConfig)
- `Config` → `DefaultConfig` (reads from Tauri app data dir)

`ConnectionService` trait (defined in `core-runtime`) → `SshConnectionService`.

### Database

SQLite at `<app_data_dir>/jackpot.db`. WAL mode, foreign keys enabled. `Database::execute()` is the only way to access the connection — never expose `MutexGuard`. Schema tracked in `_schema_version` table; migrations are incremental: v1 (hosts + config tables), v2 (known_hosts table).

### Keepalive & Reconnect

`spawn_keepalive(session, interval_secs)` — periodic `session.keepalive_send()`; disconnects on failure.

`spawn_reconnect(session, policy, get_config)` — exponential backoff (`base_delay * 2^(attempt-1)`, capped at `max_delay`). `get_config` closure called per attempt to fetch fresh credentials. Emits `Reconnecting` and `ReconnectFailed` events.

### Frontend

Vue 3 Composition API (`<script setup>`). Single `App.vue` with sidebar (host list + search) and main panel (host detail / edit form / terminal). `Terminal.vue` wraps xterm.js with `FitAddon` and `ResizeObserver`.

## Key Conventions

- All code comments in **Chinese** (中文)
- Avoid `=` or `-` as comment separators
- Technical terms (CPU, RAM, SSH, PTY, SFTP, etc.) remain in English
- `unsafe` blocks annotated with `// SAFETY:` comments
- **No emojis** in code or comments
- All public event/ID types implement `Debug + Clone + Serialize + Deserialize`
- `CoreRuntime::start()` guards against double-call
- Crate names use hyphens in Cargo.toml (`core-common`), Rust code uses underscores (`core_common`)
- `tracing` for logging; `thiserror` for error types; `serde` with tag-based JSON for events
- Deps: `ssh2` 0.9, `rusqlite` 0.31 (bundled), `tokio` full, `tauri` 2
