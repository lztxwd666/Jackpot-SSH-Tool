//! SSH 连接引擎
//! 封装 ssh2::Session，管理 TCP 连接、SSH 握手、认证和断开的整个生命周期
//! 所有 SSH I/O 操作通过 emit_event 回调报告状态变化

use core_common::{ConnectionConfig, CoreResult, KnownHostsProvider};
use core_event::event::{ConnectionEvent, CoreEvent, HostKeyEvent};
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
    known_hosts: Option<Arc<dyn KnownHostsProvider>>,
}

impl SshConnection {
    /// 创建新的 SSH 连接实例，配置请求但尚未建立网络连接
    pub fn new(
        config: ConnectionConfig,
        dispatcher: Arc<dyn EventDispatcher>,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> Self {
        Self {
            config,
            session: None,
            dispatcher,
            known_hosts,
        }
    }

    /// 建立完整的 SSH 连接：TCP 连接 → SSH 握手 → HostKey 验证 → 认证
    /// 每个阶段完成后通过 dispatcher 发出对应的 ConnectionEvent
    pub fn connect(&mut self) -> CoreResult<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);

        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::Connecting {
                host: self.config.host.clone(),
                port: self.config.port,
            }));

        // TCP 连接
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let tcp = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| {
                core_common::CoreError::Internal(format!("invalid address {addr}: {e}"))
            })?,
            timeout,
        )
        .map_err(|e| {
            core_common::CoreError::Internal(format!("TCP connect to {addr} failed: {e}"))
        })?;
        tcp.set_read_timeout(Some(timeout)).map_err(|e| {
            core_common::CoreError::Internal(format!("set read timeout failed: {e}"))
        })?;

        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::TcpConnected));

        // SSH 握手
        let mut session = Session::new().map_err(|e| {
            core_common::CoreError::Internal(format!("create SSH session failed: {e}"))
        })?;
        session.set_tcp_stream(tcp);

        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::HandshakeStarted));
        session
            .handshake()
            .map_err(|e| core_common::CoreError::Internal(format!("SSH handshake failed: {e}")))?;

        // HostKey 验证
        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::HostKeyVerifying));

        let host_key_info = check_host_key(&session, &self.config.host, self.config.port)?;

        if let Some(ref known_hosts) = self.known_hosts {
            match known_hosts.find_host_key(&self.config.host, self.config.port)? {
                None => {
                    self.dispatcher
                        .dispatch(CoreEvent::HostKey(HostKeyEvent::Unknown {
                            host: self.config.host.clone(),
                            fingerprint: host_key_info.fingerprint.clone(),
                        }));
                }
                Some(stored) => {
                    if stored.fingerprint == host_key_info.fingerprint {
                        self.dispatcher
                            .dispatch(CoreEvent::HostKey(HostKeyEvent::Accepted));
                    } else {
                        self.dispatcher
                            .dispatch(CoreEvent::HostKey(HostKeyEvent::Rejected));
                        return Err(core_common::CoreError::Internal(format!(
                            "host key verification failed for {}: fingerprint mismatch",
                            self.config.host
                        )));
                    }
                }
            }
        }

        // 认证
        authenticate(&session, &self.config.username, &self.config.auth_method)?;
        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::Authenticated));

        self.session = Some(session);
        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::Ready));

        tracing::info!(host = %self.config.host, port = self.config.port, "SSH connection established");
        Ok(())
    }

    /// 断开 SSH 连接，释放底层 session
    pub fn disconnect(&mut self) -> CoreResult<()> {
        if let Some(session) = self.session.take() {
            drop(session);
            self.dispatcher
                .dispatch(CoreEvent::Connection(ConnectionEvent::Disconnected));
            tracing::info!(host = %self.config.host, "SSH connection closed");
        }
        Ok(())
    }

    /// 检查连接是否活跃
    pub fn is_connected(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| s.authenticated())
            .unwrap_or(false)
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
