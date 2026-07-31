# Jackpot SSH Tool

基于 Rust + Tauri v2 + Vue 3 构建的桌面 SSH 客户端。

## 功能

- **主机管理** — 保存、搜索、分组管理 SSH 主机，支持收藏
- **SSH 终端** — 基于 xterm.js 的交互式终端，支持 PTY 尺寸自适应
- **心跳保活** — 自动发送 keepalive 维持长连接
- **断线重连** — 指数退避策略的自动重连

## 开发

```bash
# 环境要求: Rust 1.95.0 (通过 rustup 安装), Node.js >= 20

# 构建与测试
cargo build
cargo test
cargo clippy --all-targets -- -D warnings

# 前端
cd crates/desktop/ui && npm install
cd crates/desktop/ui && npm run type-check

# 启动桌面应用
npm run dev
```

## 架构

```
desktop ──→ core-runtime ──→ core-event
                │                 │
                └──→ core-common ←┘
                │
                └──→ core-storage
```

详见 `CLAUDE.md` 了解完整架构与开发规范。

## 许可

MIT
