# AGENTS.md

## Project

Jackpot SSH Tool — desktop SSH client. Rust core + Tauri v2 + Vue 3 frontend. Currently Phase 1 (Foundation): skeleton with event framework, no SSH code yet.

## Architecture

Layered, top-down only. Core crates must never import Tauri.

```
desktop ──→ core-runtime ──→ core-event
                │                 │
                └──→ core-common ←┘
                │
                └──→ core-storage
```

Full architecture spec at `docs/core_idea_zh/` (Chinese) and `docs/core_idea_en/` (English). Read `01_Architecture` and `09_Engineering_Guidelines` before significant changes.

## Crate Map

| Crate          | Dir                    | Purpose                                                  |
| -------------- | ---------------------- | -------------------------------------------------------- |
| `core-common`  | `crates/core-common/`  | `CoreError`, ID newtypes, `Config` trait, `init_logging` |
| `core-event`   | `crates/core-event/`   | `CoreEvent` enum, `ChannelDispatcher` (broadcast)        |
| `core-runtime` | `crates/core-runtime/` | `CoreRuntime`, `Service`/`Repository`/`Provider` traits  |
| `core-storage` | `crates/core-storage/` | SQLite via `Database`, schema migrations                 |
| `desktop`      | `crates/desktop/`      | Tauri v2 app, IPC commands, Vue 3 frontend               |

## Commands

`rust-toolchain.toml` pins Rust to 1.95.0. System `rustc` (apt) is 1.75 — too old. Ensure rustup binaries are first in `PATH`:

```
PATH="$HOME/.cargo/bin:$PATH"
```

Then run normally:

```
cargo build                      # workspace-wide
cargo test                       # 4 tests (core-event + core-storage)
cargo clippy --all-targets -- -D warnings
```

WSL low on memory? Limit parallel jobs: `CARGO_BUILD_JOBS=1 cargo build`

Root-level:

```
npm run dev      # Tauri dev server (starts Vue + Rust)
npm run build    # Vue production build
```

Frontend lives at `crates/desktop/ui/`, **not** at repo root:

```
cd crates/desktop/ui && npm install
cd crates/desktop/ui && npm run build
cd crates/desktop/ui && npm run type-check
```

## Testing

```
cargo test                         # workspace-wide (4 tests: core-event + core-storage)
cargo test -p core-storage         # storage tests only (creates temp SQLite DBs)
cargo test -p core-event           # event tests only
```

Tests in `core-storage` create temp directories with real SQLite files. They clean up on success but may leave artifacts on failure in `$TMPDIR`.

## Key Conventions

- All code comments in **Chinese** (中文)
- Avoid `=` or `-` as comment separators
- Technical terms (CPU, RAM, I2C, SSD1309, RSS) remain in English
- `unsafe` blocks annotated with `// SAFETY:` comments
- **No emojis** in code or comments
- All public event/ID types implement `Debug + Clone + Serialize + Deserialize`
- `Database::execute()` is the only way to access the SQLite connection — never expose `MutexGuard`
- `CoreRuntime::start()` guards against double-call
- Crate names use hyphens in Cargo.toml (`core-common`), Rust code uses underscores (`core_common`)
- `tracing` for logging; `thiserror` for error types; `serde` with tag-based JSON for events

## Event Flow

```
CoreRuntime → ChannelDispatcher (broadcast) → desktop/lib.rs event loop → tauri.emit("core-event") → Vue listen("core-event")
```

Add new event variants to `CoreEvent` enum in `core-event/src/event.rs`. Events are immutable, describe things that already happened, and carry minimal payloads.
