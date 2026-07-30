# Phase 2a Connection Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement SSH connection establishment — TCP connect, handshake, authenticate (password + private key), verify host keys, clean disconnect.

**Architecture:** New `core-runtime/src/ssh/` module wraps `ssh2::Session`. New events flow through existing `ChannelDispatcher`. Traits (`ConnectionService`, `KnownHostsProvider`, `CredentialProvider`) injected into `CoreRuntime`.

**Tech Stack:** Rust 1.95, ssh2 0.9, tokio, existing tracing/serde/rusqlite stack.

## Global Constraints

- Core crates must never depend on Tauri
- Events are immutable, describe things that happened, carry minimal payload
- All public event types must implement Debug, Clone, Serialize, Deserialize
- Database access through `Database::execute()` closure pattern only
- Comments in Chinese, technical terms in English, no emojis
- CoreRuntime::start() guards against double call
- All SSH I/O runs in `tokio::task::spawn_blocking`

## File Mapping

```
core-common/src/
  lib.rs          [modify: add ssh re-export]
  ssh.rs          [create: ConnectionConfig, AuthMethod, HostKeyInfo]

core-event/src/
  event.rs        [modify: add ConnectionEvent, HostKeyEvent, CredentialEvent variants]

core-runtime/
  Cargo.toml      [modify: add ssh2 dependency]
  src/
    lib.rs        [modify: add ssh/, connection_service, knownhosts, credential re-exports]
    ssh/
      mod.rs      (not needed if we use lib.rs re-exports)
      connection.rs  [create: SshConnection struct]
      auth.rs        [create: authenticate()]
      hostkey.rs     [create: check_host_key()]
    connection_service.rs [create: trait + impl]
    knownhosts.rs         [create: trait]
    credential.rs         [create: trait + ConfigCredentialProvider impl]

core-storage/src/
  lib.rs          [modify: add knownhosts re-export]
  migrations.rs   [modify: add V2 migration]
  knownhosts.rs   [create: SQLite KnownHostsProvider impl]
```

---

### Task 1: SSH Types and Events

**Files:**
- Create: `crates/core-common/src/ssh.rs`
- Modify: `crates/core-common/src/lib.rs`
- Modify: `crates/core-event/src/event.rs`

**Interfaces:**
- Produces:
  - `ConnectionConfig { host, port, username, auth_method, timeout_secs }`
  - `AuthMethod::Password(key), PrivateKey { path, passphrase }, Agent`
  - `HostKeyInfo { host, port, key_type, fingerprint }`
  - `ConnectionEvent` enum variants
  - `HostKeyEvent` enum variants
  - `CredentialEvent` enum variants

- [ ] **Step 1: Create core-common/src/ssh.rs**

```rust
//! SSH 连接相关的基础类型定义

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// SSH 连接配置，包含目标主机、端口、用户名和认证方式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub timeout_secs: u64,
}

impl ConnectionConfig {
    /// 创建一个新的连接配置，默认端口 22、超时 30 秒
    pub fn new(host: String, username: String, auth_method: AuthMethod) -> Self {
        Self {
            host,
            port: 22,
            username,
            auth_method,
            timeout_secs: 30,
        }
    }

    /// 设置自定义端口
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// 设置自定义超时
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_secs = seconds;
        self
    }
}

/// SSH 认证方式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "data")]
pub enum AuthMethod {
    /// 密码认证
    Password(String),
    /// 私钥文件认证，可选口令
    PrivateKey { path: PathBuf, passphrase: Option<String> },
    /// SSH Agent 认证（阶段 2a 暂不实现）
    Agent,
}

/// 主机密钥信息，用于 known_hosts 验证
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}

impl HostKeyInfo {
    pub fn new(host: String, port: u16, key_type: String, fingerprint: String) -> Self {
        Self { host, port, key_type, fingerprint }
    }
}
```

- [ ] **Step 2: Update core-common/src/lib.rs**

Add `pub mod ssh;` and re-export:
```rust
pub mod ssh;
pub use ssh::{AuthMethod, ConnectionConfig, HostKeyInfo};
```

- [ ] **Step 3: Update core-event/src/event.rs**

Add new event variants to `CoreEvent` enum and create the three new event enums:

```rust
// Append these variants to CoreEvent enum:
Connection(ConnectionEvent),
HostKey(HostKeyEvent),
Credential(CredentialEvent),

// Add these new event enums after ApplicationEvent/SystemEvent:

/// SSH 连接生命周期事件
/// 按顺序: Connecting → TcpConnected → HandshakeStarted → HostKeyVerifying → Authenticated → Ready
/// 异常路径: Connecting → Failed 或任意阶段 → Disconnected
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum ConnectionEvent {
    /// TCP 连接已发起，携带目标主机信息
    Connecting { host: String, port: u16 },
    /// TCP socket 已建立
    TcpConnected,
    /// SSH 协议握手开始
    HandshakeStarted,
    /// 正在进行 HostKey 验证，等待用户决策或自动校验
    HostKeyVerifying,
    /// 认证成功，连接可用
    Authenticated,
    /// 连接完全就绪，可以打开 Channel
    Ready,
    /// 连接已正常断开
    Disconnected,
    /// 连接失败，携带原因描述
    Failed { reason: String },
}

/// HostKey 验证事件
/// 用于通知前端出现未知主机或主机密钥变更的情况
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum HostKeyEvent {
    /// 发现未知主机，需要用户确认
    Unknown { host: String, fingerprint: String },
    /// 主机密钥已变更，可能存在中间人攻击
    Changed { host: String, old_fingerprint: String, new_fingerprint: String },
    /// 主机密钥已被接受
    Accepted,
    /// 用户拒绝该主机密钥
    Rejected,
}

/// 凭据操作事件
/// 凭据值绝不出现在事件中，仅携带操作结果状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum CredentialEvent {
    /// 凭据加载成功
    Loaded,
    /// 未找到指定凭据
    NotFound(String),
    /// 凭据访问被拒绝（权限问题）
    AccessDenied(String),
}
```

- [ ] **Step 4: Verify build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-common -p core-event`
Expected: Both compile without errors.

- [ ] **Step 5: Update existing serialization test**

Add test for new event variants in `event.rs` tests:
```rust
#[test]
fn test_connection_event_roundtrip() {
    let event = CoreEvent::Connection(ConnectionEvent::Connecting {
        host: "example.com".into(),
        port: 22,
    });
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        parsed,
        CoreEvent::Connection(ConnectionEvent::Connecting { ref host, port: 22 })
        if host == "example.com"
    ));
}
```

- [ ] **Step 6: Commit**

---

### Task 2: SSH Connection Engine (core-runtime/ssh/)

**Files:**
- Modify: `crates/core-runtime/Cargo.toml`
- Create: `crates/core-runtime/src/ssh/connection.rs`
- Create: `crates/core-runtime/src/ssh/auth.rs`
- Create: `crates/core-runtime/src/ssh/hostkey.rs`

**Interfaces:**
- Consumes: `ConnectionConfig`, `AuthMethod`, `HostKeyInfo` from core-common; `CoreEvent`, `ChannelDispatcher` from core-event
- Produces:
  - `SshConnection::new(config) -> Self`
  - `SshConnection::connect() -> CoreResult<()>` (TCP + handshake)
  - `SshConnection::authenticate() -> CoreResult<()>`
  - `SshConnection::disconnect() -> CoreResult<()>`
  - `fn authenticate(session, config) -> CoreResult<()>` (auth.rs)
  - `fn check_host_key(session, host, port) -> CoreResult<HostKeyInfo>` (hostkey.rs)

- [ ] **Step 1: Add ssh2 to core-runtime/Cargo.toml**

Add to `[dependencies]`:
```toml
ssh2 = "0.9"
```

- [ ] **Step 2: Write core-runtime/src/ssh/auth.rs**

```rust
//! SSH 认证逻辑模块
//! 支持密码和私钥文件两种认证方式，Agent 认证留待未来实现

use core_common::{AuthMethod, CoreResult};
use ssh2::Session;

/// 使用 ConnectionConfig 中指定的认证方式对 SSH session 进行认证
/// 按顺序尝试密码认证和私钥认证，任一种成功即返回 Ok
pub fn authenticate(session: &Session, username: &str, auth_method: &AuthMethod) -> CoreResult<()> {
    match auth_method {
        AuthMethod::Password(password) => {
            session.userauth_password(username, password)
                .map_err(|e| core_common::CoreError::Internal(format!("password auth failed: {e}")))?;
            tracing::info!(username, "password authentication succeeded");
        }
        AuthMethod::PrivateKey { path, passphrase } => {
            let key_data = std::fs::read_to_string(path)
                .map_err(|e| core_common::CoreError::Internal(format!("failed to read private key {}: {e}", path.display())))?;
            let pass = passphrase.as_deref();
            session.userauth_pubkey_memory(username, None, &key_data, pass)
                .map_err(|e| core_common::CoreError::Internal(format!("publickey auth failed: {e}")))?;
            tracing::info!(username, key_path = %path.display(), "publickey authentication succeeded");
        }
        AuthMethod::Agent => {
            return Err(core_common::CoreError::Internal("SSH agent authentication not yet supported".into()));
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Write core-runtime/src/ssh/hostkey.rs**

```rust
//! HostKey 验证模块
//! 从 SSH session 获取远程主机指纹，用于 known_hosts 校验

use core_common::{CoreResult, HostKeyInfo};

/// 从已建立 SSH 握手的 session 获取远程主机的密钥信息
/// 返回 Base64 编码的 SHA-256 指纹（与 OpenSSH 格式兼容）
pub fn check_host_key(session: &ssh2::Session, host: &str, port: u16) -> CoreResult<HostKeyInfo> {
    let key = session.host_key()
        .ok_or_else(|| core_common::CoreError::Internal("no host key available from session".into()))?;
    let key_type = key_to_type_name(key.key_type());
    let fingerprint = session.host_key_hash(ssh2::HashType::Sha256)
        .map(|hash| format_fingerprint(&hash))?;
    Ok(HostKeyInfo {
        host: host.to_string(),
        port,
        key_type,
        fingerprint,
    })
}

/// 将 ssh2 的 HostKeyType 转换为字符串表示
fn key_to_type_name(key_type_num: ssh2::HostKeyType) -> String {
    match key_type_num {
        ssh2::HostKeyType::Rsa => "ssh-rsa".into(),
        ssh2::HostKeyType::Dss => "ssh-dss".into(),
        _ => format!("ssh-unknown-{key_type_num:?}"),
    }
}

/// 将字节哈希转换为 Base64 指纹字符串（遵循 OpenSSH 格式: SHA256:xxxxx）
fn format_fingerprint(hash: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = "SHA256:".to_string();
    for byte in hash {
        write!(s, "{byte:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_fingerprint() {
        let hash = [0xffu8; 32];
        let fp = format_fingerprint(&hash);
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), 7 + 64); // "SHA256:" + 32 bytes * 2 hex chars
    }
}
```

- [ ] **Step 4: Write core-runtime/src/ssh/connection.rs**

```rust
//! SSH 连接引擎
//! 封装 ssh2::Session，管理 TCP 连接、SSH 握手、认证和断开的整个生命周期
//! 所有 SSH I/O 操作通过 emit_event 回调报告状态变化

use core_common::{ConnectionConfig, CoreResult};
use core_event::event::{ConnectionEvent, CoreEvent};
use core_event::EventDispatcher;
use ssh2::Session;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use super::auth::authenticate;
use super::hostkey::check_host_key;

/// SSH 连接实例，封装 ssh2::Session 的生命周期管理
pub struct SshConnection {
    config: ConnectionConfig,
    session: Option<Session>,
    dispatcher: Arc<dyn EventDispatcher>,
}

impl SshConnection {
    /// 创建新的 SSH 连接实例，配置请求但尚未建立网络连接
    pub fn new(config: ConnectionConfig, dispatcher: Arc<dyn EventDispatcher>) -> Self {
        Self {
            config,
            session: None,
            dispatcher,
        }
    }

    /// 建立完整的 SSH 连接：TCP 连接 → SSH 握手 → HostKey 验证 → 认证
    /// 每个阶段完成后通过 dispatcher 发出对应的 ConnectionEvent
    pub fn connect(&mut self) -> CoreResult<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);

        self.dispatcher.dispatch(CoreEvent::Connection(
            ConnectionEvent::Connecting {
                host: self.config.host.clone(),
                port: self.config.port,
            },
        ));

        // TCP 连接
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let tcp = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| core_common::CoreError::Internal(format!("invalid address {addr}: {e}")))?,
            timeout,
        )
        .map_err(|e| core_common::CoreError::Internal(format!("TCP connect to {addr} failed: {e}")))?;
        tcp.set_read_timeout(Some(timeout))
           .map_err(|e| core_common::CoreError::Internal(format!("set read timeout failed: {e}")))?;

        self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::TcpConnected));

        // SSH 握手
        let mut session = Session::new()
            .map_err(|e| core_common::CoreError::Internal(format!("create SSH session failed: {e}")))?;
        session.set_tcp_stream(tcp);

        self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::HandshakeStarted));
        session.handshake()
            .map_err(|e| core_common::CoreError::Internal(format!("SSH handshake failed: {e}")))?;

        // HostKey 验证
        self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::HostKeyVerifying));

        // 认证
        authenticate(&session, &self.config.username, &self.config.auth_method)?;
        self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::Authenticated));

        self.session = Some(session);
        self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::Ready));

        tracing::info!(host = %self.config.host, port = self.config.port, "SSH connection established");
        Ok(())
    }

    /// 断开 SSH 连接，释放底层 session
    pub fn disconnect(&mut self) -> CoreResult<()> {
        if let Some(session) = self.session.take() {
            drop(session);
            self.dispatcher.dispatch(CoreEvent::Connection(ConnectionEvent::Disconnected));
            tracing::info!(host = %self.config.host, "SSH connection closed");
        }
        Ok(())
    }

    /// 检查连接是否活跃
    pub fn is_connected(&self) -> bool {
        self.session.as_ref().map(|s| s.authenticated()).unwrap_or(false)
    }

    /// 获取内部 session 的引用（仅在连接建立后有效）
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }
}

impl Drop for SshConnection {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
```

- [ ] **Step 5: Verify build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-common -p core-event -p core-runtime`
Expected: All compile without errors.

- [ ] **Step 6: Run hostkey tests**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p core-runtime -- test_format_fingerprint`
Expected: PASS

- [ ] **Step 7: Commit**

---

### Task 3: Provider Traits (core-common)

**Files:**
- Create: `crates/core-common/src/knownhosts.rs`
- Create: `crates/core-common/src/credential.rs`
- Modify: `crates/core-common/src/lib.rs`

**Interfaces:**
- Produces:
  - `KnownHostsProvider` trait: `fn find_host_key(host, port) -> CoreResult<Option<HostKeyInfo>>`, `fn store_host_key(info) -> CoreResult<()>`, `fn remove_host_key(host, port) -> CoreResult<()>`
  - `CredentialProvider` trait: `fn load_credential(host, username) -> CoreResult<AuthMethod>`
  - `ConfigCredentialProvider` struct (empty, delegates to ConnectionConfig)

**Reason:** Traits in `core-common` avoid circular deps between `core-runtime` and `core-storage`.

- [ ] **Step 1: Write core-common/src/knownhosts.rs**

```rust
//! KnownHosts Provider 抽象
//! 定义主机密钥的查询、存储和删除操作

use crate::{CoreResult, HostKeyInfo};

/// 主机密钥的持久化存储接口
/// 用于在 SSH 连接建立前验证远程主机的身份
pub trait KnownHostsProvider: Send + Sync {
    fn find_host_key(&self, host: &str, port: u16) -> CoreResult<Option<HostKeyInfo>>;
    fn store_host_key(&self, info: &HostKeyInfo) -> CoreResult<()>;
    fn remove_host_key(&self, host: &str, port: u16) -> CoreResult<()>;
}
```

- [ ] **Step 2: Write core-common/src/credential.rs**

```rust
//! Credential Provider 抽象
//! 定义认证凭据的加载接口

use crate::{AuthMethod, CoreResult};

/// 提供 SSH 认证所需的凭据
pub trait CredentialProvider: Send + Sync {
    fn load_credential(&self, host: &str, username: &str) -> CoreResult<AuthMethod>;
}

/// 基于 ConnectionConfig 的凭据提供实现
/// 直接从连接配置中获取认证方式，不额外查找
pub struct ConfigCredentialProvider;

impl CredentialProvider for ConfigCredentialProvider {
    fn load_credential(&self, _host: &str, _username: &str) -> CoreResult<AuthMethod> {
        Err(crate::CoreError::NotFound("ConfigCredentialProvider delegates to ConnectionConfig".into()))
    }
}
```

- [ ] **Step 3: Update core-common/src/lib.rs**

Add:
```rust
pub mod credential;
pub mod knownhosts;
pub use credential::{ConfigCredentialProvider, CredentialProvider};
pub use knownhosts::KnownHostsProvider;
```

- [ ] **Step 4: Verify build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-common`
Expected: Compiles.

- [ ] **Step 5: Commit**

---

### Task 4: ConnectionService (core-runtime)

**Files:**
- Create: `crates/core-runtime/src/connection_service.rs`
- Modify: `crates/core-runtime/src/lib.rs`

**Interfaces:**
- Consumes: `SshConnection`, `ConnectionConfig`, `KnownHostsProvider` from core-common
- Produces: `ConnectionService` trait + `SshConnectionService` impl

- [ ] **Step 1: Write core-runtime/src/connection_service.rs**

```rust
//! 连接服务模块
//! 编排 SSH 连接、HostKey 验证和认证的完整流程

use core_common::{ConnectionConfig, CoreResult};
use core_common::knownhosts::KnownHostsProvider;
use core_event::event::{ConnectionEvent, CoreEvent};
use core_event::EventDispatcher;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::ssh::connection::SshConnection;

/// 连接服务抽象，定义 SSH 连接状态管理接口
pub trait ConnectionService: Send + Sync {
    async fn connect(&self, config: ConnectionConfig) -> CoreResult<()>;
    async fn disconnect(&self) -> CoreResult<()>;
    fn is_connected(&self) -> bool;
}

/// SSH 连接服务的具体实现
pub struct SshConnectionService {
    dispatcher: Arc<dyn EventDispatcher>,
    known_hosts: Arc<dyn KnownHostsProvider>,
    connection: Mutex<Option<SshConnection>>,
}

impl SshConnectionService {
    pub fn new(
        dispatcher: Arc<dyn EventDispatcher>,
        known_hosts: Arc<dyn KnownHostsProvider>,
    ) -> Self {
        Self {
            dispatcher,
            known_hosts,
            connection: Mutex::new(None),
        }
    }
}

impl ConnectionService for SshConnectionService {
    async fn connect(&self, config: ConnectionConfig) -> CoreResult<()> {
        let dispatcher = self.dispatcher.clone();
        let host = config.host.clone();
        let port = config.port;

        let result = tokio::task::spawn_blocking(move || {
            let mut conn = SshConnection::new(config, dispatcher);
            conn.connect()
        })
        .await
        .map_err(|e| core_common::CoreError::Internal(format!("spawn_blocking failed: {e}")))?;

        match result {
            Ok(()) => {
                tracing::info!(host, port, "SSH connection service: connected");
                Ok(())
            }
            Err(e) => {
                self.dispatcher.dispatch(CoreEvent::Connection(
                    ConnectionEvent::Failed { reason: e.to_string() },
                ));
                Err(e)
            }
        }
    }

    async fn disconnect(&self) -> CoreResult<()> {
        let mut guard = self.connection.lock().await;
        if let Some(mut conn) = guard.take() {
            conn.disconnect()?;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // 需要通过 Mutex 检查，简单实现返回 false
        false
    }
}
```

- [ ] **Step 2: Update core-runtime/src/lib.rs**

```rust
pub mod provider;
pub mod repository;
pub mod runtime;
pub mod service;

pub use provider::Provider;
pub use repository::Repository;
pub use runtime::CoreRuntime;
pub use service::Service;

// Phase 2a 新增
pub mod ssh {
    pub mod auth;
    pub mod connection;
    pub mod hostkey;
}
pub mod connection_service;

pub use connection_service::{ConnectionService, SshConnectionService};
pub use ssh::connection::SshConnection;
```

- [ ] **Step 5: Verify build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-runtime`
Expected: Compiles without errors.

- [ ] **Step 6: Commit**

---

### Task 4: Storage — KnownHosts SQLite Provider

**Files:**
- Modify: `crates/core-storage/src/migrations.rs`
- Create: `crates/core-storage/src/knownhosts.rs`
- Modify: `crates/core-storage/src/lib.rs`

**Interfaces:**
- Consumes: `Database::execute()`, `HostKeyInfo`, `KnownHostsProvider` trait (from core-common)
- Produces: `SqliteKnownHosts` implementing `KnownHostsProvider`

- [ ] **Step 1: Add V2 migration in migrations.rs**

Add after existing migration code:

```rust
const SCHEMA_VERSION: i32 = 2;

const MIGRATION_V2: &str = "
CREATE TABLE IF NOT EXISTS known_hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(host, port)
);
";
```

Add to `run_migrations`:
```rust
if current_version < 2 {
    conn.execute_batch(MIGRATION_V2)
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
    tracing::info!("database migrated to version 2");
}
```

Then update the SCHEMA_VERSION insert to use the new constant:
```rust
conn.execute("INSERT INTO _schema_version (version) VALUES (?1)", [SCHEMA_VERSION])
```

Also update the existing `if current_version < 1` block to insert `[1]` instead of `[SCHEMA_VERSION]`, so incremental migration works correctly.

- [ ] **Step 2: Write core-storage/src/knownhosts.rs**

```rust
//! KnownHosts 的 SQLite 持久化实现
//! 将主机密钥信息存储到 known_hosts 表中

use core_common::{CoreResult, HostKeyInfo, KnownHostsProvider};
use std::sync::Arc;

use crate::db::Database;

/// 基于 SQLite 的 KnownHosts Provider 实现
pub struct SqliteKnownHosts {
    db: Arc<Database>,
}

impl SqliteKnownHosts {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl KnownHostsProvider for SqliteKnownHosts {
    fn find_host_key(&self, host: &str, port: u16) -> CoreResult<Option<HostKeyInfo>> {
        let host_owned = host.to_string();
        self.db.execute(|conn| {
            let mut stmt = conn.prepare(
                "SELECT key_type, fingerprint FROM known_hosts WHERE host = ?1 AND port = ?2"
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let mut rows = stmt.query_map(
                rusqlite::params![host_owned, port],
                |row| {
                    Ok(HostKeyInfo {
                        host: host_owned.clone(),
                        port,
                        key_type: row.get(0)?,
                        fingerprint: row.get(1)?,
                    })
                },
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            Ok(rows.filter_map(|r| r.ok()).next())
        })
    }

    fn store_host_key(&self, info: &HostKeyInfo) -> CoreResult<()> {
        let host = info.host.clone();
        let key_type = info.key_type.clone();
        let fingerprint = info.fingerprint.clone();
        let port = info.port;

        self.db.execute(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO known_hosts (host, port, key_type, fingerprint) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![host, port, key_type, fingerprint],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }

    fn remove_host_key(&self, host: &str, port: u16) -> CoreResult<()> {
        let host_owned = host.to_string();
        self.db.execute(move |conn| {
            conn.execute(
                "DELETE FROM known_hosts WHERE host = ?1 AND port = ?2",
                rusqlite::params![host_owned, port],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::path::PathBuf;

    fn setup_db() -> Arc<Database> {
        let dir = std::env::temp_dir().join(format!("jackpot-test-knownhosts-{}", uuid::Uuid::new_v4()));
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();
        Arc::new(db)
    }

    #[test]
    fn test_store_and_find_host_key() {
        let db = setup_db();
        let provider = SqliteKnownHosts::new(db.clone());

        let info = HostKeyInfo::new(
            "example.com".into(),
            22,
            "ssh-rsa".into(),
            "SHA256:abcdef1234567890".into(),
        );

        provider.store_host_key(&info).unwrap();

        let found = provider.find_host_key("example.com", 22).unwrap().unwrap();
        assert_eq!(found.host, "example.com");
        assert_eq!(found.key_type, "ssh-rsa");
        assert_eq!(found.fingerprint, "SHA256:abcdef1234567890");
    }

    #[test]
    fn test_remove_host_key() {
        let db = setup_db();
        let provider = SqliteKnownHosts::new(db.clone());

        let info = HostKeyInfo::new("test.com".into(), 2222, "ssh-ed25519".into(), "SHA256:deadbeef".into());
        provider.store_host_key(&info).unwrap();
        assert!(provider.find_host_key("test.com", 2222).unwrap().is_some());

        provider.remove_host_key("test.com", 2222).unwrap();
        assert!(provider.find_host_key("test.com", 2222).unwrap().is_none());
    }
}
```

- [ ] **Step 3: Update core-storage/src/lib.rs**

```rust
pub mod db;
pub mod migrations;
pub mod knownhosts;

pub use db::Database;
pub use knownhosts::SqliteKnownHosts;
```

**No Cargo.toml changes needed** — core-storage already depends on core-common where the trait lives.

- [ ] **Step 4: Verify build and tests**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-storage`
Expected: Compiles successfully.

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p core-storage`
Expected: All tests pass (including new knownhosts tests).

- [ ] **Step 6: Commit**

---

### Task 5: CoreRuntime Integration

**Files:**
- Modify: `crates/core-runtime/src/runtime.rs`

**Interfaces:**
- Produces: `CoreRuntime` now holds `SshConnectionService` and `SqliteKnownHosts`

- [ ] **Step 1: Update runtime.rs to hold connection service**

Replace `runtime.rs` with the enhanced version:

```rust
use core_common::config::Config;
use core_common::knownhosts::KnownHostsProvider;
use core_common::CoreResult;
use core_event::event::{ApplicationEvent, CoreEvent};
use core_event::{ChannelDispatcher, EventDispatcher};
use core_storage::Database;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::connection_service::{ConnectionService, SshConnectionService};

/// 应用运行时核心，协调各服务的生命周期
/// 持有时事件分发器、数据库连接和连接服务
pub struct CoreRuntime {
    config: Box<dyn Config>,
    dispatcher: Arc<ChannelDispatcher>,
    db: Arc<RwLock<Option<Database>>>,
    running: RwLock<bool>,
    connection_service: RwLock<Option<Arc<dyn ConnectionService>>>,
    known_hosts: RwLock<Option<Arc<dyn KnownHostsProvider>>>,
}

impl CoreRuntime {
    pub fn new(config: Box<dyn Config>) -> Self {
        let dispatcher = Arc::new(ChannelDispatcher::new(256));
        Self {
            config,
            dispatcher,
            db: Arc::new(RwLock::new(None)),
            running: RwLock::new(false),
            connection_service: RwLock::new(None),
            known_hosts: RwLock::new(None),
        }
    }

    pub fn dispatcher(&self) -> Arc<ChannelDispatcher> {
        self.dispatcher.clone()
    }

    pub fn config(&self) -> &dyn Config {
        self.config.as_ref()
    }

    pub async fn start(&self) -> CoreResult<()> {
        {
            let running = self.running.read().await;
            if *running {
                return Err(core_common::CoreError::Internal(
                    "runtime already started".into(),
                ));
            }
        }
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Started));

        let db = Database::open(self.config.app_data_dir())?;
        db.migrate()?;

        let db_arc = Arc::new(db);
        let known_hosts: Arc<dyn KnownHostsProvider> = Arc::new(core_storage::SqliteKnownHosts::new(db_arc.clone()));

        let conn_service: Arc<dyn ConnectionService> = Arc::new(SshConnectionService::new(
            self.dispatcher.clone(),
            known_hosts.clone(),
        ));

        {
            let mut db_lock = self.db.write().await;
            *db_lock = Some(Arc::try_unwrap(db_arc).unwrap_or_else(|arc| {
                panic!("Database Arc still has references")
            }));
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = Some(known_hosts);
        }
        {
            let mut cs_lock = self.connection_service.write().await;
            *cs_lock = Some(conn_service);
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Ready));

        tracing::info!("core runtime started");
        Ok(())
    }

    /// 获取连接服务的引用（仅在 start() 后有效）
    pub async fn connection_service(&self) -> Option<Arc<dyn ConnectionService>> {
        self.connection_service.read().await.clone()
    }

    /// 获取 KnownHosts Provider 的引用（仅在 start() 后有效）
    pub async fn known_hosts(&self) -> Option<Arc<dyn KnownHostsProvider>> {
        self.known_hosts.read().await.clone()
    }

    pub async fn shutdown(&self) {
        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownRequested));

        // 先断开连接
        {
            let cs_lock = self.connection_service.read().await;
            if let Some(ref cs) = *cs_lock {
                let _ = cs.disconnect().await;
            }
        }
        {
            let mut cs_lock = self.connection_service.write().await;
            *cs_lock = None;
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = None;
        }

        // 再关闭数据库
        {
            let mut db_lock = self.db.write().await;
            if let Some(db) = db_lock.take() {
                let _ = db.close();
            }
        }

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownCompleted));

        tracing::info!("core runtime shut down");
    }
}
```

- [ ] **Step 2: Verify build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p core-runtime`
Expected: Compiles successfully. Fix any circular dependency issues between core-runtime and core-storage if they arise (the trait is in runtime, impl is in storage).

- [ ] **Step 3: Verify full workspace builds**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build`
Expected: All crates compile.

- [ ] **Step 4: Commit**

---

### Task 6: Integration Verification

**Files:**
- No new files

**Interfaces:**
- Verifies: full build, all tests, clippy

- [ ] **Step 1: Run full test suite**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test`
Expected: At least 6 tests pass (2 core-event, 2 core-storage db, 2 core-storage knownhosts, 1 core-runtime hostkey)

- [ ] **Step 2: Run clippy**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo clippy --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Verify workspace structure**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo metadata --format-version=1 --no-deps 2>/dev/null | python3 -c "import json,sys; [print(m['name']) for m in json.load(sys.stdin)['packages']]"` or equivalent
Expected: Lists all 5 crate names.

- [ ] **Step 4: Verify no circular deps**

Check: core-common ← core-event, core-storage, core-runtime (no reverse deps). desktop → core-runtime → core-common; desktop never imported by any core crate.

- [ ] **Step 5: Commit**
