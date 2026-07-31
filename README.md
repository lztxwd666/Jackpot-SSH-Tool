# Jackpot SSH Tool

Desktop SSH client built with Rust + Tauri v2 + Vue 3.

## Features

- **Host Management** — Save, search, and organize SSH hosts with groups and favorites
- **SSH Terminal** — Interactive terminal via xterm.js with PTY resize support
- **Keepalive** — Automatic keepalive to maintain persistent connections
- **Reconnect** — Exponential backoff reconnection after disconnection

## Development

```bash
# Prerequisites: Rust 1.95.0 (via rustup), Node.js >= 20
PATH="$HOME/.cargo/bin:$PATH"

# Build and test
cargo build
cargo test
cargo clippy --all-targets -- -D warnings

# Frontend
cd crates/desktop/ui && npm install
cd crates/desktop/ui && npm run type-check

# Launch (Windows)
npm run dev
```

## Architecture

```
desktop ──→ core-runtime ──→ core-event
                │                 │
                └──→ core-common ←┘
                │
                └──→ core-storage
```

See `CLAUDE.md` for detailed architecture and `docs/core_idea_en/` for full design specs.

## License

MIT
