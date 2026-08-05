# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Jackpot SSH Tool — desktop SSH client. Rust workspace (Tauri v2 + Vue 3). Currently functional: host CRUD in SQLite, SSH connection with password/key auth, keepalive, manual reconnect (frontend-initiated; auto-reconnect removed by product decision), and xterm.js terminal in the UI.

## Build, Test & Verification

`rust-toolchain.toml` pins Rust to 1.95.0. Install via [rustup](https://rustup.rs/).

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

# Tauri dev server (starts Vue dev server + Rust backend + desktop window)
npm run dev

# Frontend (inside crates/desktop/ui/ — NOT at repo root)
cd crates/desktop/ui && npm install
cd crates/desktop/ui && npm run build
cd crates/desktop/ui && npm run type-check
```

Tests in `core-storage` create temp directories with real SQLite files. They clean up on success but may leave artifacts on failure in `%TEMP%`.

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

| Crate          | Dir                    | Purpose                                                                                                                                                              | Key Types                                                                                                                                                                                                                                                                                   |
| -------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core-common`  | `crates/core-common/`  | Shared types, error enum, ID newtypes (UUID via `define_id!` macro), trait definitions                                                                               | `CoreError`, `Host`, `HostId`/`SessionId`/`ChannelId`, `ConnectionConfig`, `AuthMethod`, `SessionState`, `ChannelType`, `ChannelState`, `PtySize`, `HostKeyInfo`, `Config`, `HostRepository`, `KnownHostsProvider`, `CredentialProvider`, `Credential`, `CredentialKind`                   |
| `core-event`   | `crates/core-event/`   | Immutable event definitions, broadcast channel dispatcher                                                                                                            | `CoreEvent` (tag-based JSON via `#[serde(tag = "type")]`), `ChannelDispatcher`, `ApplicationEvent`, `SystemEvent`, `ConnectionEvent`, `HostKeyEvent`, `SessionEvent`, `ChannelEvent`, `HostEvent`                                                                                            |
| `core-runtime` | `crates/core-runtime/` | Application lifecycle, session/channel management, SSH engine (worker message model: keepalive, SFTP and transfers all run inside the per-session worker; reconnect is manual only) | `CoreRuntime`, `Session`, `Channel`, `SshConnection`, `SshConnectionService`, `ConnectionService` trait, `WorkerCommand`, `WorkerHandle`, `KeyringCredentialProvider`                                                                                                                       |
| `core-storage` | `crates/core-storage/` | SQLite + WAL, schema migrations, `Database::execute()` is the sole DB access gate                                                                                    | `Database`, `SqliteHostRepository`, `SqliteKnownHosts`                                                                                                                                                                                                                                      |
| `desktop`      | `crates/desktop/`      | Tauri v2 app, IPC bridge, Vue 3 + xterm.js terminal UI                                                                                                               | `AppState`, Tauri commands                                                                                                                                                                                                                                                                  |

### SSH Connection Pipeline

```
SshConnection::connect()
  ├─ TCP connect (TcpStream::connect_timeout)
  ├─ ssh2::Session::handshake()
  ├─ host_key check → HostKeyEvent (Unknown/Accepted/Rejected)
  ├─ authenticate() → password | pubkey_file | Agent(未实现) 认证
  └─ Ready event
```

`SshConnection` holds `Option<ssh2::Session>`; disconnects on `Drop`. `Session` owns no ssh2 resources: every SSH operation is posted as a `WorkerCommand` to the per-session worker thread (Active Object model) via `WorkerHandle`, with oneshot replies. The worker serializes connect/disconnect/exec/channel/SFTP/transfer operations, sends keepalives, and polls shell reads (`do_idle_work`). Reconnect is manual only (frontend-initiated); no reconnect machinery lives in the worker.

`Channel` is a pure handle (id + channel_type + session_id + worker handle). The actual `ssh2::Channel` (Shell with PTY) and `ssh2::Sftp` live inside the worker's `raw_channels` registry. Blocking calls post commands and wait for oneshot replies (usually from `tokio::task::spawn_blocking`). Shell output is emitted as `DataReceived` events by the worker's idle poll loop; `Channel::start_read_loop()` is a compatibility no-op.

### Event Flow

```
CoreRuntime → ChannelDispatcher (broadcast) → desktop/lib.rs event loop → tauri.emit("core-event") → Vue listen("core-event")
```

Event JSON is tag-based: `{"type": "Channel", "payload": {"kind": "DataReceived", "detail": {"channel_id": "...", "data": [...]}}}`. The Vue `Terminal` component filters by `channel_id` and writes bytes to xterm. User keystrokes go back via `terminal_send_input` IPC command.

Add new event variants to `CoreEvent` enum in `core-event/src/event.rs`. Events are immutable, describe things that already happened, and carry minimal payloads.

### Tauri IPC Commands

All commands in `desktop/src/commands.rs`. Shared state: `AppState { runtime, channels }` (via `tauri::State`).

| Command                                                     | Purpose                                                                                              |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `get_app_status`                                            | Returns "running" if CoreRuntime is initialized                                                      |
| `ping`                                                      | Health check, returns "pong"                                                                         |
| `list_hosts` / `save_host` / `delete_host` / `search_hosts` | Host CRUD                                                                                            |
| `create_session`                                            | Creates Session, returns session_id                                                                  |
| `connect_session`                                           | Calls `Session::connect()` in spawn_blocking                                                         |
| `open_shell`                                                | Opens Shell + SFTP channels, returns channel_id (read loop is worker-driven)                         |
| `terminal_send_input`                                       | Writes bytes to channel                                                                              |
| `terminal_resize`                                           | Resizes PTY (cols, rows)                                                                             |
| `terminal_close`                                            | Closes session, removes channels                                                                     |
| `load_credential` / `save_credential` / `delete_credential` | OS credential manager CRUD (`kind`: "password" \| "passphrase")                                      |
| `ping_host`                                                 | Ping diagnostic via system ping (`-n 1` on Windows / `-c 1` elsewhere), returns success + latency_ms |

### Trait Abstractions

Core traits defined in `core-common`, implemented in `core-storage`:

- `HostRepository` → `SqliteHostRepository` (hosts table)
- `KnownHostsProvider` → `SqliteKnownHosts` (known_hosts table)
- `CredentialProvider` → `KeyringCredentialProvider` (OS credential manager via keyring; storage key: service `jackpot-ssh`, user `{kind}:{host}:{port}:{username}`)
- `Config` → `DefaultConfig` (reads from Tauri app data dir)

`ConnectionService` trait (defined in `core-runtime`) → `SshConnectionService`.

### Database

SQLite at `<app_data_dir>/jackpot.db`. WAL mode, foreign keys enabled. `Database::execute()` is the only way to access the connection — never expose `MutexGuard`. Schema tracked in `_schema_version` table; migrations are incremental: v1 (hosts + config tables), v2 (known_hosts table), v3 (hosts.save_password flag column; the password itself lives in the OS credential manager, never SQLite).

### Keepalive & Reconnect

Keepalive lives inside the worker's idle work (`do_idle_work`); the standalone `spawn_keepalive`/`spawn_reconnect` tasks were removed in Stage 6.

Keepalive: the worker sends `keepalive_send()` every 30s of idle time; on failure it disconnects and broadcasts `SessionEvent::Disconnected` (which carries a `reason` field). Disconnect-on-error semantics: any real channel I/O error (read/write, non-EAGAIN) also triggers an active disconnect with reason, so a dead connection always surfaces a `Disconnected` event.

Reconnect: manual only — the user decision is "never reconnect proactively". The worker never triggers reconnect; auto-reconnect machinery (state machine, `ReconnectPolicy`, `CredentialEvent`, `provide_reconnect_credential`) was fully removed. After a `Disconnected` event, the frontend may re-initiate via `connect_session` (re-supplying credentials) and `open_shell`.

### Frontend

Vue 3 Composition API (`<script setup>`). `App.vue` hosts the tab workspace (wrap tab bar + per-tab terminal/SFTP panels + status bar at the left area bottom) and the right-hand host sidebar (host list + search + slide-out form panel + right-click menu). Tabs are matched by `hostId` (name is not unique and can be renamed). `Terminal.vue` wraps xterm.js with `FitAddon` and `ResizeObserver`.

## Key Conventions

- All code comments in **Chinese** (中文)
- Avoid `=` or `-` as comment separators
- Technical terms (CPU, RAM, SSH, PTY, SFTP, etc.) remain in English
- `unsafe` is allowed but minimized: some operations (raw FFI, custom layouts) are impossible without it — that is a Rust limitation, not an error. Every `unsafe` block MUST be annotated with a `// SAFETY:` comment explaining the invariants that make it sound. Prefer safe wrappers that encapsulate `unsafe` behind a narrow, well-tested interface. Zero `unsafe` remains the goal for new code when a safe alternative exists
- **No emojis** in code or comments
- All public event/ID types implement `Debug + Clone + Serialize + Deserialize`
- `CoreRuntime::start()` guards against double-call
- Crate names use hyphens in Cargo.toml (`core-common`), Rust code uses underscores (`core_common`)
- `tracing` for logging; `thiserror` for error types; `serde` with tag-based JSON for events
- Deps: `ssh2` 0.9, `rusqlite` 0.31 (bundled), `tokio` full, `tauri` 2

## Fix Quality Bar

- Fix defects with the best trade-off of **safety, extensibility, performance and structure** — never the quickest patch. Consider how the fix serves future features and long-term maintenance before committing to it
- **Avoid over-engineering**: no speculative abstraction, no framework-before-need. The right fix is the smallest change that eliminates the defect's *class*, not just its instance
- When a fix would duplicate existing logic, that is a signal the shared logic belongs in a shared place — extract and reuse instead of copying
- A fix that forks an existing primitive (e.g. a second retry loop beside `io_retry`) creates two truths to maintain; prefer one well-placed primitive used by all callers
- Balance is the goal: a structural fix beats a patch; a few lines in the right place beat a framework

## UI Language Rules

- **Language division of labor**: UI/UX strings are English (default) or Chinese (user-selectable) for broad user reach; the developer's working language is Chinese — code comments, commit messages, and internal documentation stay in Chinese per project convention
- UI supports **English (en) and Chinese (zh)** out of the box, switchable via the sidebar footer selector; the choice persists in localStorage
- ALL user-visible strings (buttons, menus, toasts, dialogs, placeholders, titles) MUST go through `t()` from `crates/desktop/ui/src/composables/i18n.ts` — never hardcode UI strings in components
- Adding a language requires: (1) a new locale entry in `messages` in `i18n.ts`, (2) adding the locale to the `Locale` type and the sidebar selector
- `currentLocale` is a Vue ref — switching language re-renders all components automatically
- File contents, file names, and SSH output are NOT UI strings — do not translate them
- Code comments still follow the Chinese convention above
