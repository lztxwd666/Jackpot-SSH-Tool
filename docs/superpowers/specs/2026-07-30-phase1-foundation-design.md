# Phase 1 Foundation Design

> Stage 1 of the roadmap: build the project skeleton.

## Tech Stack

| Layer           | Technology                |
| --------------- | ------------------------- |
| Frontend        | Vue 3 + TypeScript + Vite |
| Desktop         | Tauri v2                  |
| Async Runtime   | tokio                     |
| Logging         | tracing                   |
| Database        | rusqlite + SQLite         |
| Serialization   | serde + serde_json        |
| Package Manager | npm                       |

## Crate Structure (Approach A — Minimal for Phase 1)

```
jackpot-ssh-tool/
├── crates/
│   ├── jackpot-core-common/      # Shared types: Error, ID types, Config trait
│   ├── jackpot-core-event/       # CoreEvent framework: enum, Dispatcher trait
│   ├── jackpot-core-runtime/     # Runtime orchestrator, Service/Repository/Provider traits
│   ├── jackpot-core-storage/     # SQLite init, migrations, Repository impls
│   └── jackpot-tauri/            # Tauri v2 app, IPC adapter, Vue frontend
├── Cargo.toml                    # Workspace root
├── package.json
└── docs/
```

When Phase 2 (SSH Core) arrives, `jackpot-core-runtime` will be split into:
`jackpot-core-domain`, `jackpot-core-service`, `jackpot-core-repository`, `jackpot-core-provider`.

## Dependency Graph (Top-Down)

```
jackpot-tauri ──→ jackpot-core-runtime ──→ jackpot-core-event
                       │                        │
                       └──→ jackpot-core-common ←┘
                       │
                       └──→ jackpot-core-storage ──→ jackpot-core-common
```

Core never depends on Tauri. Tauri depends on Core. Events flow upward, commands flow downward.

## Phase 1 Deliverables

| #   | Deliverable             | Location                       |
| --- | ----------------------- | ------------------------------ |
| 1   | Cargo workspace         | `Cargo.toml` (root)            |
| 2   | Core common types       | `crates/jackpot-core-common/`  |
| 3   | CoreEvent framework     | `crates/jackpot-core-event/`   |
| 4   | Core Runtime traits     | `crates/jackpot-core-runtime/` |
| 5   | SQLite initialization   | `crates/jackpot-core-storage/` |
| 6   | Logging (tracing)       | `crates/jackpot-core-common/`  |
| 7   | Error handling          | `crates/jackpot-core-common/`  |
| 8   | Configuration loading   | `crates/jackpot-core-runtime/` |
| 9   | Tauri app scaffold      | `crates/jackpot-tauri/`        |
| 10  | Vue + TypeScript + Vite | `crates/jackpot-tauri/ui/`     |
| 11  | IPC command stub        | `crates/jackpot-tauri/src/`    |
| 12  | Event forwarding stub   | `crates/jackpot-tauri/src/`    |
| 13  | Unit test framework     | Each crate                     |

## Key Design Decisions

- **No SSH code in Phase 1.** The milestone is "app starts, runtime runs, events can be dispatched."
- **Service/Repository/Provider are traits** in Phase 1. Concrete impls come in Phase 2+.
- **CoreEvent is a single enum** for now, with `ApplicationEvent` and `SystemEvent` variants.
- **SQLite schema** includes only a `_migrations` table and a `hosts` table stub.
- **Tauri adapter** has stub commands that call through to Core Runtime.
