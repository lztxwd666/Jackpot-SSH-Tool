# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Jackpot SSH Tool — desktop SSH client. Rust workspace (Tauri v2 + Vue 3). Currently functional: host CRUD in SQLite (groups, favorites with star toggle in the sidebar), SSH connection with password/key auth, keepalive, manual reconnect (frontend-initiated; auto-reconnect removed by product decision), xterm.js terminal with custom right-click menu (copy/paste/clear), and a dual file tree (local/remote) with: single-file and recursive folder transfers (drag & drop, right-click, Ctrl/Shift multi-select batch operations), name-conflict resolution (auto-rename/overwrite, system file-manager style), new file/folder creation, SHA-256 verification for single files, and a VSCode-style header with hover actions.

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

**Strict one-way dependencies apply at every level, not just crates**: module-level cycles within a crate are forbidden. A module must not import from a module that (transitively) imports it — in Rust, `use` compiles either way, so cycles are silent design debt. When extracting shared logic, place shared raw types in the module that owns them (e.g. `ChannelInner` lives in `worker`, `open_shell_raw` with it; `channel` only imports `worker`, never the reverse). Frontend composables follow the same rule (`fs` → `dialog` → `i18n` is fine; `dialog` → `fs` would not be). After dependency-affecting changes run `python scripts/check_rust_deps.py` (Rust module graph) and confirm the frontend import graph stays acyclic.

### Crate Map

| Crate          | Dir                    | Purpose                                                                                                                                                                             | Key Types                                                                                                                                                                                                                                                |
| -------------- | ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core-common`  | `crates/core-common/`  | Shared types, error enum, ID newtypes (UUID via `define_id!` macro), trait definitions                                                                                              | `CoreError`, `Host`, `HostId`/`SessionId`/`ChannelId`, `ConnectionConfig`, `AuthMethod`, `SessionState`, `ChannelType`, `PtySize`, `HostKeyInfo`, `Config`, `HostRepository`, `KnownHostsProvider`, `CredentialProvider`, `Credential`, `CredentialKind` |
| `core-event`   | `crates/core-event/`   | Immutable event definitions, broadcast channel dispatcher                                                                                                                           | `CoreEvent` (tag-based JSON via `#[serde(tag = "type")]`), `ChannelDispatcher`, `ApplicationEvent`, `SystemEvent`, `ConnectionEvent`, `HostKeyEvent`, `SessionEvent`, `ChannelEvent`, `HostEvent`                                                        |
| `core-runtime` | `crates/core-runtime/` | Application lifecycle, session/channel management, SSH engine (worker message model: keepalive, SFTP and transfers all run inside the per-session worker; reconnect is manual only) | `CoreRuntime`, `Session`, `Channel`, `SshConnection`, `SshConnectionService`, `ConnectionService` trait, `WorkerCommand`, `WorkerHandle`, `KeyringCredentialProvider`                                                                                    |
| `core-storage` | `crates/core-storage/` | SQLite + WAL, schema migrations, `Database::execute()` is the sole DB access gate                                                                                                   | `Database`, `SqliteHostRepository`, `SqliteKnownHosts`                                                                                                                                                                                                   |
| `desktop`      | `crates/desktop/`      | Tauri v2 app, IPC bridge, Vue 3 + xterm.js terminal UI                                                                                                                              | `AppState`, Tauri commands                                                                                                                                                                                                                               |

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

`Channel` is a pure handle (id + channel_type + session_id + worker handle). The actual `ssh2::Channel` (Shell with PTY) and `ssh2::Sftp` live inside the worker's `raw_channels` registry (`ChannelInner` is owned by `worker`, keeping `channel` → `worker` one-way). Blocking calls post commands and wait for oneshot replies (usually from `tokio::task::spawn_blocking`). Shell output is emitted as `DataReceived` events by the worker's idle poll loop.

Transfers: single-file and recursive folder transfer share the same core (`transfer_one_download`/`transfer_one_upload`, progress via callback); `run_transfer`/`run_transfer_cmd` wrap Locked/Unlocked events, nested-transfer rejection, and deferred disconnect flush (`pending_disconnect`/`pending_close`, released after the transfer stack pops to avoid use-after-free). Folder transfers enumerate entries first (skipping symlinks), then transfer file-by-file with aggregate progress carrying the current relative path. Progress channel is `(u64, u64, String)` (done, total, current filename). Disconnect cancels transfers at every file/chunk boundary.

### Event Flow

```
CoreRuntime → ChannelDispatcher (broadcast) → desktop/lib.rs event loop → tauri.emit("core-event") → Vue listen("core-event")
```

Event JSON is tag-based: `{"type": "Channel", "payload": {"kind": "DataReceived", "detail": {"channel_id": "...", "data": "<base64>"}}}` (byte payloads are base64-encoded via `serde_with` to cut IPC bandwidth). The Vue `Terminal` component filters by `channel_id` and writes bytes to xterm. User keystrokes go back via `terminal_send_input` IPC command.

Add new event variants to `CoreEvent` enum in `core-event/src/event.rs`. Events are immutable, describe things that already happened, and carry minimal payloads.

### Tauri IPC Commands

All commands in `desktop/src/commands/` (module per area). Shared state: `AppState { runtime, channels, sftp_channels }` (via `tauri::State`). `channels` maps shell channel ids; `sftp_channels` maps session id → sftp channel. Passwords travel only as IPC arguments, never in events or SQLite.

| Command                                                                                                                                     | Purpose                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `get_app_status` / `ping`                                                                                                                   | Runtime status check / health check                                                                      |
| `list_hosts` / `save_host` / `delete_host` / `search_hosts`                                                                                 | Host CRUD (favorite flag persisted with host)                                                            |
| `approve_host_key`                                                                                                                          | Store host key after TOFU confirmation (key_type passed through; V4 matching requires real type)         |
| `get_home_dir` / `read_local_dir` / `read_local_file` / `write_local_file` / `rename_local_file` / `delete_local_file` / `create_local_dir` | Local file system (blocking IO wrapped in `spawn_blocking`; read_local_file capped at 10MB)              |
| `create_session` / `connect_session` / `open_shell`                                                                                         | Session lifecycle (open_shell opens shell + sftp channels together)                                      |
| `terminal_send_input` / `terminal_resize` / `terminal_close` / `start_terminal`                                                             | Terminal I/O (start_terminal is a compatibility no-op)                                                   |
| `load_credential` / `save_credential` / `delete_credential`                                                                                 | OS credential manager CRUD (`kind`: "password" \| "passphrase")                                          |
| `ping_host`                                                                                                                                 | Ping diagnostic via system ping (`-n 1` Windows / `-c 1` elsewhere), target validated against `-` prefix |
| `sftp_list_dir` / `sftp_create_dir` / `sftp_create_file` / `sftp_delete` / `sftp_rename`                                                    | Remote file tree operations                                                                              |
| `sftp_download_file` / `sftp_upload_file`                                                                                                   | Single-file transfer with SHA-256 verification (verifying state pushed to progress event)                |
| `sftp_download_tree` / `sftp_upload_tree`                                                                                                   | Recursive folder transfer (aggregate progress + current filename; no per-file verification)              |

Transfers stream in Rust (`spawn_blocking`), never through IPC payloads. Progress events (`transfer-progress`) carry `{ id, done, total, verifying, filename }`.

### Trait Abstractions

Core traits defined in `core-common`, implemented in `core-storage`:

- `HostRepository` → `SqliteHostRepository` (hosts table)
- `KnownHostsProvider` → `SqliteKnownHosts` (known_hosts table)
- `CredentialProvider` → `KeyringCredentialProvider` (OS credential manager via keyring; storage key: service `jackpot-ssh`, user `{kind}:{host}:{port}:{username}`)
- `Config` → `DefaultConfig` (pure injection: app data dir and log level supplied by the caller)

`ConnectionService` trait (defined in `core-runtime`) → `SshConnectionService`.

### Database

SQLite at `<app_data_dir>/jackpot.db`. WAL mode, foreign keys enabled. `Database::execute()` is the only way to access the connection — never expose `MutexGuard`. Schema tracked in `_schema_version` table; migrations are incremental and run inside a transaction (DDL + version row are atomic): v1 (hosts + config tables), v2 (known_hosts table), v3 (hosts.save_password flag column; the password itself lives in the OS credential manager, never SQLite), v4 (known_hosts unique key becomes `(host, port, key_type)` so multiple key types coexist; legacy rows get timestamp format normalized). `find_host_key` matches by key_type, so `approve_host_key` must store the real type (never a placeholder).

### Keepalive & Reconnect

Keepalive lives inside the worker's idle work (`do_idle_work`); the standalone `spawn_keepalive`/`spawn_reconnect` tasks were removed in Stage 6.

Keepalive: the worker sends `keepalive_send()` every 30s of idle time (timing starts at connection establishment); the ssh2 session must be configured with `set_keepalive(true, 30)` at connect time, otherwise libssh2's default interval of 0 makes `keepalive_send` a no-op. On failure it disconnects and broadcasts `SessionEvent::Disconnected` (which carries a `reason` field). Disconnect-on-error semantics: any real channel I/O error (read/write, non-EAGAIN) also triggers an active disconnect with reason, so a dead connection always surfaces a `Disconnected` event.

Reconnect: manual only — the user decision is "never reconnect proactively". The worker never triggers reconnect; auto-reconnect machinery (state machine, `ReconnectPolicy`, `CredentialEvent`, `provide_reconnect_credential`) was fully removed. After a `Disconnected` event, the frontend may re-initiate via `connect_session` (re-supplying credentials) and `open_shell`.

### Frontend

Vue 3 Composition API (`<script setup>`). `App.vue` hosts the tab workspace (tab bar + per-tab `SessionTab` + status bar) and the right-hand host sidebar (host list with search, favorite stars, hover info card, right-click menu, slide-out form panel). Tabs are matched by `hostId` (name is not unique and can be renamed).

Per-tab status is a notices model: `TabNotice[]` (id / level / message) upserted and removed by the event dispatcher — new notices are a new event mapping, rendering is unchanged; action buttons live in the disconnect overlay, never in the status bar. The connect flow is protected by a 30s timeout (TCP connect timeout is 15s) with a cancel token checked at every await, an abandoned-sessions set filters late `Connected` events, and closing a tab cancels in-flight reconnects and transfers (progress bar removed immediately, late results silenced).

File trees implement VSCode-style multi-select (`selected: reactive(Set)` + anchor): Ctrl toggles, Shift range-selects, plain click selects (double-click enters folders), right-click is selection-aware, and batch operations (upload/download/delete N items) are emitted as `download-many`/`upload-many` item arrays that `App.vue` executes serially (a single worker forbids concurrent transfers). Selection clears on outside clicks (`useClearSelectionOnOutside`) and on tree blank clicks. Drag payloads are JSON `{ items: [{path, isDir}] }` (legacy plain-path strings still parse). Reusable frontend composables live in `crates/desktop/ui/src/composables/` (`fs`, `pos`, `menu`, `selection`, `dialog`, `i18n`).

File/folder type icons resolve through a theme interface (`composables/fileIcon.ts` + `FileIcon.vue`): built-in material theme with assets under `assets/icons/material/` (MIT, sourced from vscode-material-icon-theme; upstream LICENSE copied in; mapping and curated subset generated by `scripts/fetch_material_icons.py` — re-run it to update icons, do not hand-edit `manifest.ts` or the SVG files). Theme switching is reserved (`registerFileIconTheme` / `setFileIconTheme`, reactive `currentThemeId`); only the material theme ships today, tree folders are drill-down so the `open` state is unused but supported by the interface.

`Terminal.vue` wraps xterm.js with `FitAddon` and `ResizeObserver`; hidden tabs skip fit to avoid 1x1 PTY resizes; EOF (remote exit) stops input and shows `[Session ended]`; right-click shows a custom menu (copy/paste/clear).

## Key Conventions

- All code comments in **Chinese** (中文)
- **Comments contain substantive content only**: state facts or rationale about the code; no conversational or colloquial phrasing (e.g. "只做我们有能力实现的功能"), no filler words — if a comment cannot be written substantively, omit it
- Avoid `=` or `-` as comment separators
- Technical terms (CPU, RAM, SSH, PTY, SFTP, etc.) remain in English
- `unsafe` is allowed but minimized: some operations (raw FFI, custom layouts) are impossible without it — that is a Rust limitation, not an error. Every `unsafe` block MUST be annotated with a `// SAFETY:` comment explaining the invariants that make it sound. Prefer safe wrappers that encapsulate `unsafe` behind a narrow, well-tested interface. Zero `unsafe` remains the goal for new code when a safe alternative exists
- **No emojis** in code or comments
- All public event/ID types implement `Debug + Clone + Serialize + Deserialize`
- `CoreRuntime::start()` guards against double-call
- Crate names use hyphens in Cargo.toml (`core-common`), Rust code uses underscores (`core_common`)
- `tracing` for logging; `thiserror` for error types; `serde` with tag-based JSON for events
- Deps: `ssh2` 0.9, `rusqlite` 0.31 (bundled), `tokio` full, `tauri` 2
- Git commit messages must NOT include a `Co-Authored-By` trailer (product decision, do not add Claude as a co-author)

## Icon Sources

**File/folder type icons** come from [vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) (MIT). This is the only permitted source for type icons unless a new source is explicitly approved; any third-party icon set used must be bundled with its license and attributed.

- Assets live in `crates/desktop/ui/src/assets/icons/themes/{theme}/` (`file/`, `folder/` with `-open` variants, `LICENSE`, `manifest.ts`). The material theme is curated, not the full upstream set (~130 file + ~130 folder SVGs, ~150KB raw) — coverage target is common daily file types, with unknown types falling back to the theme default icon
- **Single source of truth**: the curated mapping and copy list live in `scripts/fetch_material_icons.py`; it regenerates SVGs, open variants, and `manifest.ts`. Never hand-edit `manifest.ts` or the SVG files — change the script and re-run (`python scripts/fetch_material_icons.py [upstream_path]`)
- Attribution is mandatory: the upstream `LICENSE` must ship with the assets, and `manifest.ts` records the upstream commit
- **Adding a new icon theme**: (1) create `assets/icons/themes/{name}/` with the same structure, its own `LICENSE` and manifest; (2) register a `FileIconTheme` (`id`, `assetsDir`, `resolveFile`, `resolveFolder`) in `composables/fileIcon.ts` and extend the `import.meta.glob` directory list there; (3) add the theme to the setting persistence key space when the settings UI lands (reserved: `registerFileIconTheme` / `setFileIconTheme`, reactive `currentThemeId`). Ship a curated subset, not a full icon set — bundle size matters

**Fixed action icons** (file-tree header new-file/new-folder/refresh buttons, lock banner, star, etc.) come from [vscode-codicons](https://github.com/microsoft/vscode-codicons) (MIT, the same icon set VSCode itself uses; paths inlined in the components with attribution comments). They are intentionally NOT theme-switchable — action icons must stay recognizable regardless of the type-icon theme.

## Fix Quality Bar

- Fix defects with the best trade-off of **safety, extensibility, performance and structure** — never the quickest patch. Consider how the fix serves future features and long-term maintenance before committing to it
- **Avoid over-engineering**: no speculative abstraction, no framework-before-need. The right fix is the smallest change that eliminates the defect's *class*, not just its instance
- When a fix would duplicate existing logic, that is a signal the shared logic belongs in a shared place — extract and reuse instead of copying
- A fix that forks an existing primitive (e.g. a second retry loop beside `io_retry`) creates two truths to maintain; prefer one well-placed primitive used by all callers
- Balance is the goal: a structural fix beats a patch; a few lines in the right place beat a framework

## New File Guidelines

- **Prefer extending existing modules over creating new files**: a new file is a new home for logic; before creating one, verify the logic does not belong in an existing module (Fix Quality Bar: shared logic belongs in a shared place). Creating a file is a structural decision, not an organizational convenience
- **One file = one cohesive responsibility**, named by responsibility (snake_case, e.g. `retry.rs`, `fs.ts`) — never file-per-feature or file-per-component speculation
- **Every new file opens with a `//!` module doc** (Chinese) stating the file's responsibility and, where non-obvious, the design decision behind it
- **New utility/composable functions must have ≥2 callers or be part of a clear abstraction**; a single-caller helper stays private in the caller's module
- **Match the crate's existing idiom**: same comment density, same naming, same error handling (thiserror/`CoreError`), same logging (`tracing`), same CSS variable conventions for frontend
- **New public types implement `Debug + Clone + Serialize + Deserialize`** where applicable (events/IDs per Key Conventions); secrets must never be serializable (see `AuthMethod`)
- **Key Conventions apply unchanged to new files**: Chinese comments, no emojis, no `=`/`-` comment separators; frontend user-visible strings via `t()` with en/zh pairs
- **UI behavior follows established product conventions** (VSCode/OS file manager patterns) — do not invent novel interaction patterns where a mainstream one exists
- **Verify before reporting**: new code must pass build + test + clippy (`--all-targets -- -D warnings`) and frontend type-check + build

## UI Language Rules

- **Language division of labor**: UI/UX strings are English (default) or Chinese (user-selectable) for broad user reach; the developer's working language is Chinese — code comments, commit messages, and internal documentation stay in Chinese per project convention
- UI supports **English (en) and Chinese (zh)** out of the box, switchable via the sidebar footer selector; the choice persists in localStorage
- ALL user-visible strings (buttons, menus, toasts, dialogs, placeholders, titles) MUST go through `t()` from `crates/desktop/ui/src/composables/i18n.ts` — never hardcode UI strings in components
- Adding a language requires: (1) a new locale entry in `messages` in `i18n.ts`, (2) adding the locale to the `Locale` type and the sidebar selector
- `currentLocale` is a Vue ref — switching language re-renders all components automatically
- File contents, file names, and SSH output are NOT UI strings — do not translate them
- Code comments still follow the Chinese convention above
