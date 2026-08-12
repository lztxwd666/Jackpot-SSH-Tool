//! SSH 连接引擎
//! 封装 ssh2::Session，管理 TCP 连接、SSH 握手、认证和断开的整个生命周期
//! 所有 SSH I/O 操作通过 emit_event 回调报告状态变化

use core_common::{ConnectionConfig, CoreResult, KnownHostsProvider};
use core_event::EventDispatcher;
use core_event::event::{ConnectionEvent, CoreEvent, HostKeyEvent};
use ssh2::Session;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
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
    /// 失败时统一分发 ConnectionEvent::Failed 后返回错误
    pub fn connect(&mut self) -> CoreResult<()> {
        let result = self.connect_inner();
        if let Err(e) = &result {
            self.dispatcher
                .dispatch(CoreEvent::Connection(ConnectionEvent::Failed {
                    reason: e.to_string(),
                }));
        }
        result
    }

    fn connect_inner(&mut self) -> CoreResult<()> {
        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::Connecting {
                host: self.config.host.clone(),
                port: self.config.port,
            }));

        // TCP 连接：主机名经系统 DNS 解析（(&str, u16) 的 ToSocketAddrs 实现把 IP 字面量
        // 直接解析、其余走 getaddrinfo，IPv6 字面量同样正确处理）。解析出的地址逐个
        // 尝试连接、首个成功即用（OpenSSH 同款行为：多地址按序尝试，前一个不可达回退
        // 下一个）。DNS 解析在 worker 线程内阻塞执行（Active Object 语义：连接管线
        // 全部在 worker，不卡 UI）
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let addrs = (self.config.host.as_str(), self.config.port)
            .to_socket_addrs()
            .map_err(|e| {
                core_common::CoreError::Internal(format!(
                    "cannot resolve host {}: {e}",
                    self.config.host
                ))
            })?;
        let mut tcp = None;
        let mut last_err: Option<std::io::Error> = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(s) => {
                    tcp = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let tcp = tcp.ok_or_else(|| {
            last_err.map_or_else(
                || {
                    core_common::CoreError::Internal(format!(
                        "no usable address for {}",
                        self.config.host
                    ))
                },
                |e| {
                    core_common::CoreError::Internal(format!(
                        "TCP connect to {}:{} failed: {e}",
                        self.config.host, self.config.port
                    ))
                },
            )
        })?;
        tcp.set_read_timeout(Some(timeout)).map_err(|e| {
            core_common::CoreError::Internal(format!("set read timeout failed: {e}"))
        })?;
        // 禁用 Nagle 算法，确保键盘输入小包立即发送
        tcp.set_nodelay(true)
            .map_err(|e| core_common::CoreError::Internal(format!("set nodelay failed: {e}")))?;

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
            // 按 key_type 匹配：同主机多密钥类型并存互不覆盖（V4 起唯一键含 key_type）
            match known_hosts.find_host_key(
                &self.config.host,
                self.config.port,
                &host_key_info.key_type,
            )? {
                None => {
                    // 未知主机密钥：发出事件并中止连接（TOFU 流程）
                    // 用户确认后通过 approve_host_key 按真实 key_type 存储密钥，然后重新连接
                    self.dispatcher
                        .dispatch(CoreEvent::HostKey(HostKeyEvent::Unknown {
                            host: self.config.host.clone(),
                            key_type: host_key_info.key_type.clone(),
                            fingerprint: host_key_info.fingerprint.clone(),
                        }));
                    return Err(core_common::CoreError::HostKeyUnknown {
                        fingerprint: host_key_info.fingerprint.clone(),
                    });
                }
                Some(stored) => {
                    if stored.fingerprint == host_key_info.fingerprint {
                        self.dispatcher
                            .dispatch(CoreEvent::HostKey(HostKeyEvent::Accepted));
                    } else {
                        // 密钥变更（可能 MITM）：发出事件并中止连接
                        self.dispatcher
                            .dispatch(CoreEvent::HostKey(HostKeyEvent::Changed {
                                host: self.config.host.clone(),
                                key_type: host_key_info.key_type.clone(),
                                old_fingerprint: stored.fingerprint.clone(),
                                new_fingerprint: host_key_info.fingerprint.clone(),
                            }));
                        return Err(core_common::CoreError::HostKeyChanged {
                            fingerprint: host_key_info.fingerprint.clone(),
                        });
                    }
                }
            }
        }

        // 认证
        authenticate(&session, &self.config.username, &self.config.auth_method)?;
        self.dispatcher
            .dispatch(CoreEvent::Connection(ConnectionEvent::Authenticated));

        // keepalive 配置：libssh2 默认 interval=0（禁用），不配置则 keepalive_send 恒为空操作；
        // 间隔与 worker 的 KEEPALIVE_INTERVAL（30s）一致，want_reply 让服务端回包确认活性
        session.set_keepalive(true, 30);

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

    /// 执行远程命令并返回 stdout
    /// 会话处于非阻塞模式：打开通道、发送请求、读取输出都可能返回 EAGAIN，
    /// 三个阶段统一走 io_retry（每阶段独立 1s 累计重试上限，检查取消标志），
    /// 不切换全局 blocking flag
    /// cancel：worker 的传输取消标志；断开置位后各阶段立即失败而非等待重试
    pub fn exec_command(
        &self,
        command: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> CoreResult<String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active session".into()))?;

        // 打开 exec 通道（非阻塞模式下可能 EAGAIN）
        let mut ch = super::retry::io_retry(|| session.channel_session(), cancel).map_err(|e| {
            core_common::CoreError::Internal(format!("open exec channel failed: {e}"))
        })?;

        // 发送 exec 请求（非阻塞模式下可能 EAGAIN）
        super::retry::io_retry(|| ch.exec(command), cancel)
            .map_err(|e| core_common::CoreError::Internal(format!("exec failed: {e}")))?;

        // 读取输出（非阻塞模式下可能 EAGAIN）
        // 累积原始字节，最后一次性解码：避免多字节 UTF-8 跨块边界被 from_utf8_lossy 损坏
        // 输出上限：exec 为公开方法，防御未来大输出场景撑爆内存（当前调用方哈希探测输出很小）
        // 单次 read 等待上限 60s：远程命令计算中无输出是正常状态（如 sha256sum 计算大文件数秒），
        // 1s 默认上限会误判失败跳过校验。注意两点（worker 单线程语义）：
        // 1) exec 执行期间命令队列不被处理，断开/关闭命令在 exec 返回后生效；
        //    cancel 预置位（如传输取消后排队的 exec）时各阶段立即失败
        // 2) 上限按单次 read 结算（每块重新计 60s 预算），sha256sum 输出集中在结尾，
        //    实际等待约等于命令计算时长，总等待无硬上限但取消可中断
        const EXEC_OUTPUT_CAP: usize = 1024 * 1024;
        const EXEC_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = super::retry::io_retry_with_cap(|| ch.read(&mut buf), cancel, EXEC_READ_WAIT)
                .map_err(|e| {
                core_common::CoreError::Internal(format!("exec read failed: {e}"))
            })?;
            if n == 0 {
                break; // EOF
            }
            if raw.len() + n > EXEC_OUTPUT_CAP {
                return Err(core_common::CoreError::Internal(
                    "exec output exceeds 1MB cap".into(),
                ));
            }
            raw.extend_from_slice(&buf[..n]);
        }
        let out = String::from_utf8(raw).map_err(|e| {
            core_common::CoreError::Internal(format!("exec output not valid UTF-8: {e}"))
        })?;
        Ok(out.trim().to_string())
    }
}

impl Drop for SshConnection {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use crate::ssh::is_would_block;
    use ssh2::{Error, ErrorCode};

    #[test]
    fn test_is_would_block_matches_libssh2_eagain() {
        // 非阻塞模式下 libssh2 返回 EAGAIN 的 Display 格式（锁定当前 ssh2 0.9.6 行为）
        let e = Error::new(
            ErrorCode::Session(-37),
            "Would block waiting for status message",
        );
        assert!(is_would_block(&e));
        let e2 = Error::new(ErrorCode::Session(-37), "would block");
        assert!(is_would_block(&e2));
    }

    #[test]
    fn test_is_would_block_rejects_real_errors() {
        let e = Error::new(ErrorCode::Session(-1), "connection refused");
        assert!(!is_would_block(&e));
        let e2 = Error::new(ErrorCode::SFTP(3), "no such file");
        assert!(!is_would_block(&e2));
    }
}
