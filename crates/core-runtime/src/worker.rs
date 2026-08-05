//! SSH 执行线程模块
//! 每 Session 一个 worker 线程，串行执行全部 ssh2 操作（Active Object 模式）
//! 事件循环: try_recv 处理命令 → 空闲工作（keepalive / shell 读轮询）→ sleep 25ms

#![allow(dead_code)]

use crate::channel::{self, ChannelInner};
use crate::ssh::SshConnection;
use core_common::{
    AuthMethod, ChannelId, ChannelType, ConnectionConfig, CoreError, CoreResult, FileEntry,
    KnownHostsProvider, ReconnectPolicy, SessionId, SessionState,
};
use core_event::EventDispatcher;
use core_event::event::{ChannelEvent, CoreEvent, CredentialEvent, SessionEvent, TransferDirection, TransferEvent};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// worker 空闲工作间隔（毫秒）：shell 读轮询与命令响应延迟的上限
const IDLE_INTERVAL_MS: u64 = 25;

/// keepalive 发送间隔：连接空闲时每 30 秒发送一次 SSH keepalive 请求以保持连接活跃
const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// SFTP 流式传输分块大小（64KB）：减少循环次数加速大文件，兼顾内存占用
const SFTP_CHUNK_SIZE: usize = 65536;

/// keepalive 间隔判定：now 距 last 达到 interval 返回 true
fn keepalive_due(
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    interval: std::time::Duration,
) -> bool {
    match last {
        None => false, // 从未发送：连接刚建立，首轮不立即发送
        Some(l) => now.duration_since(l) >= interval,
    }
}

/// 凭据等待超时：用户 60 秒内未提供凭据则放弃本轮，进入下一轮退避
const CREDENTIAL_TIMEOUT: Duration = Duration::from_secs(60);

/// 不含任何凭据的连接配置骨架（重连用，凭据由 ProvideCredential 补充）
/// Task 6 从 Session 迁入 worker：worker 是重连状态机的唯一所有者
#[derive(Clone)]
pub(crate) struct HostConnectionProfile {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: ProfileAuthKind,
    pub timeout_secs: u64,
}

#[derive(Clone)]
pub(crate) enum ProfileAuthKind {
    Password,
    PrivateKey {
        path: std::path::PathBuf,
        needs_passphrase: bool,
    },
}

/// 重连状态机纯逻辑：给定当前轮次与策略，返回本轮的退避秒数；None 表示重试耗尽
fn reconnect_delay(attempt: u32, policy: &ReconnectPolicy) -> Option<u64> {
    if attempt > policy.max_retries {
        return None;
    }
    Some(policy.delay_for(attempt))
}

/// 连接配置 → 重连用配置骨架（剥离凭据，凭据值绝不落盘）
fn profile_from_config(config: &ConnectionConfig) -> HostConnectionProfile {
    HostConnectionProfile {
        host: config.host.clone(),
        port: config.port,
        username: config.username.clone(),
        auth_kind: match &config.auth_method {
            AuthMethod::Password(_) => ProfileAuthKind::Password,
            AuthMethod::PrivateKey { path, passphrase } => ProfileAuthKind::PrivateKey {
                path: path.clone(),
                needs_passphrase: passphrase.is_some(),
            },
            AuthMethod::Agent => ProfileAuthKind::Password,
        },
        timeout_secs: config.timeout_secs,
    }
}

/// 配置骨架 → 完整配置（凭据由 ProvideCredential 补充）
fn profile_to_config(profile: &HostConnectionProfile) -> ConnectionConfig {
    ConnectionConfig::new(
        profile.host.clone(),
        profile.username.clone(),
        match &profile.auth_kind {
            ProfileAuthKind::Password => AuthMethod::Password(String::new()), // 占位，凭据到达后替换
            ProfileAuthKind::PrivateKey { path, .. } => AuthMethod::PrivateKey {
                path: path.clone(),
                passphrase: None, // 需要口令时由 ProvideCredential 注入
            },
        },
    )
    .with_port(profile.port)
    .with_timeout(profile.timeout_secs)
}

/// 远端哈希命令（探测结果缓存，会话级：同一次连接不会换操作系统）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashCommand {
    Sha256sum, // GNU/Linux、busybox/Alpine
    Shasum,    // Perl（macOS/BSD 通用）
    BsdSha256, // BSD 原生
    Openssl,   // OpenSSL 兜底
}

/// 按命令类型解析哈希输出；无法解析返回 None
pub(crate) fn parse_hash_output(cmd: &HashCommand, out: &str) -> Option<String> {
    match cmd {
        HashCommand::Sha256sum | HashCommand::Shasum => {
            out.split_whitespace().next().map(|s| s.to_string())
        }
        HashCommand::Openssl | HashCommand::BsdSha256 => {
            out.split('=').nth(1).map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
        }
    }
}

/// 按探测结果选择命令（顺序 = 跨平台覆盖概率）
fn select_hash_command(present: impl Fn(&str) -> bool) -> Option<HashCommand> {
    if present("sha256sum") {
        return Some(HashCommand::Sha256sum);
    }
    if present("shasum") {
        return Some(HashCommand::Shasum);
    }
    if present("sha256") {
        return Some(HashCommand::BsdSha256);
    }
    if present("openssl") {
        return Some(HashCommand::Openssl);
    }
    None
}

/// 会话级命令（Active Object 的 Method Request）
/// 所有变体经 mpsc 队列串行到达 worker，回执用 oneshot
pub(crate) enum WorkerCommand {
    Connect {
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    Disconnect,
    Close,
    OpenChannel {
        ctype: ChannelType,
        reply: oneshot::Sender<CoreResult<ChannelId>>,
    },
    ChannelWrite {
        channel: ChannelId,
        data: Vec<u8>,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    ChannelResize {
        channel: ChannelId,
        cols: u32,
        rows: u32,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    ChannelClose {
        channel: ChannelId,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    Exec {
        command: String,
        reply: oneshot::Sender<CoreResult<String>>,
    },
    RemoteSha256 {
        path: String,
        reply: oneshot::Sender<CoreResult<Option<String>>>,
    },
    SftpReadDir {
        path: String,
        reply: oneshot::Sender<CoreResult<Vec<FileEntry>>>,
    },
    SftpCreateDir {
        path: String,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpRemove {
        path: String,
        is_dir: bool,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpRename {
        old: String,
        new: String,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpTransfer {
        kind: TransferKind,
        remote: String,
        local: String,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
        reply: oneshot::Sender<CoreResult<u64>>,
    },
    ProvideCredential {
        secret: String,
    },
    SetReconnectPolicy {
        policy: Option<ReconnectPolicy>,
    },
    CloseAllChannels,
}

/// 重连状态（worker 线程私有）
enum ReconnectState {
    Idle,
    Backoff { attempt: u32, wake_at: Instant },
    AwaitingCredential { attempt: u32, deadline: Instant },
}

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Download,
    Upload,
}

/// worker 句柄：外部通过它投递命令
/// tx 为 UnboundedSender（Send+Sync），Session/Channel 可无锁共享
pub(crate) struct WorkerHandle {
    tx: tokio::sync::mpsc::UnboundedSender<WorkerCommand>,
    join: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl WorkerHandle {
    pub(crate) fn new(tx: tokio::sync::mpsc::UnboundedSender<WorkerCommand>) -> Self {
        Self {
            tx,
            join: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn set_join(&self, join: std::thread::JoinHandle<()>) {
        *self.join.lock().unwrap() = Some(join);
    }

    /// 投递命令（不等待回执的变体）
    pub(crate) fn send(&self, cmd: WorkerCommand) -> CoreResult<()> {
        self.tx
            .send(cmd)
            .map_err(|_| core_common::CoreError::Internal("worker thread is gone".into()))
    }

    /// 投递命令并等待回执（调用方需处于阻塞上下文，如 spawn_blocking）
    pub(crate) fn call<T>(
        &self,
        cmd: WorkerCommand,
        rx: oneshot::Receiver<CoreResult<T>>,
    ) -> CoreResult<T> {
        self.send(cmd)?;
        rx.blocking_recv()
            .map_err(|_| core_common::CoreError::Internal("worker reply channel closed".into()))?
    }
}

/// worker 主循环入口（std::thread）
/// state 与 Session 共享（外部查询状态）；重连所需配置由命令携带（后续任务扩展）
/// rx 移入 Worker 结构：传输执行期间由 transfer_*_inner 嵌套 try_recv 处理非传输命令
pub(crate) fn run_loop(
    rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    state: Arc<std::sync::RwLock<SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    session_id: SessionId,
) {
    let mut w = Worker::new(state, dispatcher, session_id, rx);
    loop {
        // 处理所有已到达的命令（传输执行中会嵌套处理，见 transfer_*_inner）
        if !w.drain_nested_commands() {
            return; // Close 完成，结束线程
        }
        w.do_idle_work();
        std::thread::sleep(std::time::Duration::from_millis(IDLE_INTERVAL_MS));
    }
}

/// worker 内部状态：全部 ssh2 相关状态移入此处（无锁，单线程独占）
struct Worker {
    state: Arc<std::sync::RwLock<SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    closed: bool,
    session_id: Option<SessionId>,     // run_loop 启动时注入
    connection: Option<SshConnection>, // ssh2 连接所有权（Task 2 引入）
    last_keepalive: Option<Instant>,   // Task 4 使用
    cancel: AtomicBool,                // Task 5 传输取消使用
    raw_channels: HashMap<ChannelId, ChannelInner>, // worker 内通道注册表
    channels: Vec<ChannelId>,          // 已打开通道 ID 列表
    rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>, // 命令队列（嵌套 try_recv 取命令）
    transferring: bool,                // 传输进行中：拒绝嵌套传输
    reconnect: ReconnectState,         // 重连状态机（Task 6 引入）
    reconnect_policy: Option<ReconnectPolicy>, // 由 Session::set_reconnect_policy 同步
    host_config: Option<HostConnectionProfile>, // 重连用配置骨架（connect 成功时保存）
    known_hosts: Option<Arc<dyn KnownHostsProvider>>, // 首次 connect 传入，重连复用
    hash_cmd: Option<HashCommand>,     // 远端哈希命令探测结果缓存（会话级）
}

impl Worker {
    fn new(
        state: Arc<std::sync::RwLock<SessionState>>,
        dispatcher: Arc<dyn EventDispatcher>,
        session_id: SessionId,
        rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    ) -> Self {
        Self {
            state,
            dispatcher,
            session_id: Some(session_id),
            connection: None,
            last_keepalive: None,
            closed: false,
            cancel: AtomicBool::new(false),
            raw_channels: HashMap::new(),
            channels: Vec::new(),
            rx,
            transferring: false,
            reconnect: ReconnectState::Idle,
            reconnect_policy: None,
            host_config: None,
            known_hosts: None,
            hash_cmd: None,
        }
    }

    /// 写入状态，守卫 Closed 终态（Closed 状态下不允许被非 Closed 覆盖）
    fn write_state(&self, s: SessionState) -> CoreResult<()> {
        let mut guard = self
            .state
            .write()
            .map_err(|e| CoreError::Internal(format!("state lock poisoned: {e}")))?;
        if *guard == SessionState::Closed && s != SessionState::Closed {
            return Err(CoreError::Internal(
                "session is closed (terminal state)".into(),
            ));
        }
        *guard = s;
        Ok(())
    }

    /// 分发事件（worker 内单线程，无需同步）
    fn dispatch(&self, event: CoreEvent) {
        self.dispatcher.dispatch(event);
    }

    /// 建立 SSH 连接（worker 内执行）
    fn connect_inner(
        &mut self,
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> CoreResult<()> {
        // 保存 known_hosts 供重连尝试复用（首次 connect 可能为 None，不覆盖已有值）
        if known_hosts.is_some() {
            self.known_hosts = known_hosts.clone();
        }
        // 配置骨架先行剥离（config 随后被移入 SshConnection）
        let profile = profile_from_config(&config);
        self.write_state(SessionState::Connecting)?;
        let host = config.host.clone();
        let port = config.port;
        self.dispatch(CoreEvent::Session(SessionEvent::Connecting {
            session_id: self.session_id(),
            host: host.clone(),
            port,
        }));
        let mut conn = SshConnection::new(config, self.dispatcher.clone(), known_hosts);
        match conn.connect() {
            Ok(()) => {
                self.connection = Some(conn);
                // 复位传输取消标志：上一次断开置位后，新连接上的传输不得被残留标志误取消
                self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                // 重置 keepalive 计时：新连接首轮不立即发送（keepalive_due(None,..) 为 false）
                self.last_keepalive = None;
                // 保存重连用配置骨架（凭据剥离；重连/凭据等待复用）
                self.host_config = Some(profile);
                // 连接成功（含重连与手动路径）即终止进行中的重连状态机：
                // 防止 Backoff 等待中手动连接成功后，wake 到点又对活连接重复建连
                self.reconnect = ReconnectState::Idle;
                self.write_state(SessionState::Connected)?;
                self.dispatch(CoreEvent::Session(SessionEvent::Connected {
                    session_id: self.session_id(),
                }));
                Ok(())
            }
            Err(e) => {
                let _ = self.write_state(SessionState::Disconnected);
                Err(e)
            }
        }
    }

    /// 断开 SSH 连接（worker 内执行），状态转为 Disconnected（可重连）
    /// 断开即断：置位取消标志，传输循环（transfer_*_inner）在下一个 chunk 检查点立即中止，
    /// 嵌套 Disconnect/Close 与正常断开共用此路径
    fn disconnect_inner(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.close_all_channels_inner();
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.disconnect();
        }
        if self.write_state(SessionState::Disconnected).is_ok() {
            self.dispatch(CoreEvent::Session(SessionEvent::Disconnected {
                session_id: self.session_id(),
            }));
        }
    }

    /// 安排重连：从第 1 次尝试进入退避状态
    /// 内部检查重连策略与 Closed 终态；由 keepalive 失败分支触发
    fn schedule_reconnect(&mut self) {
        let Some(policy) = self.reconnect_policy.clone() else {
            return; // 未设置策略：不重连
        };
        if self.closed {
            return; // Closed 终态守卫
        }
        // max_retries = 0 表示不重连：复用 reconnect_delay 的耗尽判断（None 即不重试）
        let Some(delay) = reconnect_delay(1, &policy) else {
            return;
        };
        self.reconnect = ReconnectState::Backoff {
            attempt: 1,
            wake_at: Instant::now() + Duration::from_secs(delay),
        };
    }

    /// 空闲工作中推进重连状态机（Backoff 到点 → 重连尝试 / 凭据等待 → 超时处理）
    fn advance_reconnect(&mut self) {
        // Closed 终态守卫：会话已关闭立即中止
        if self.closed {
            return;
        }
        let Some(policy) = self.reconnect_policy.clone() else {
            // 策略被移除（SetReconnectPolicy(None)）：取消进行中的重连
            self.reconnect = ReconnectState::Idle;
            return;
        };
        match &self.reconnect {
            ReconnectState::Idle => {}
            ReconnectState::Backoff { attempt, wake_at } => {
                if Instant::now() < *wake_at {
                    return; // 退避未到点
                }
                let attempt = *attempt;
                self.dispatch(CoreEvent::Session(SessionEvent::Reconnecting {
                    session_id: self.session_id(),
                    attempt,
                }));
                // 重建配置骨架（worker 保存的 host_config）
                let Some(profile) = self.host_config.clone() else {
                    self.reconnect = ReconnectState::Idle;
                    self.dispatch_reconnect_failed("no host config".into());
                    return;
                };
                let needs_secret = matches!(
                    &profile.auth_kind,
                    ProfileAuthKind::Password
                        | ProfileAuthKind::PrivateKey {
                            needs_passphrase: true,
                            ..
                        }
                );
                if needs_secret {
                    // 请求用户凭据（凭据值绝不出现在事件中）
                    self.dispatch(CoreEvent::Credential(CredentialEvent::Required {
                        session_id: self.session_id(),
                        host: profile.host.clone(),
                        username: profile.username.clone(),
                        auth_kind: match &profile.auth_kind {
                            ProfileAuthKind::Password => "password".to_string(),
                            ProfileAuthKind::PrivateKey { .. } => {
                                "private_key_passphrase".to_string()
                            }
                        },
                    }));
                    self.reconnect = ReconnectState::AwaitingCredential {
                        attempt,
                        deadline: Instant::now() + CREDENTIAL_TIMEOUT,
                    };
                    return;
                }
                self.try_connect_attempt(attempt, profile);
            }
            ReconnectState::AwaitingCredential { attempt, deadline } => {
                if Instant::now() >= *deadline {
                    // 超时：放弃本轮，进入下一轮退避
                    let next = *attempt + 1;
                    match reconnect_delay(next, &policy) {
                        Some(secs) => {
                            tracing::warn!(
                                session_id = ?self.session_id(),
                                attempt = next,
                                "reconnect credential timeout, retrying"
                            );
                            self.reconnect = ReconnectState::Backoff {
                                attempt: next,
                                wake_at: Instant::now() + Duration::from_secs(secs),
                            };
                        }
                        None => {
                            self.reconnect = ReconnectState::Idle;
                            self.dispatch_reconnect_failed("max retries exhausted".into());
                        }
                    }
                }
            }
        }
    }

    /// 按配置骨架发起一次重连尝试（无凭据场景）
    fn try_connect_attempt(&mut self, attempt: u32, profile: HostConnectionProfile) {
        let config = profile_to_config(&profile);
        self.reconnect_connect(attempt, config);
    }

    /// 执行一次重连连接尝试：成功广播 Reconnected；失败按策略退避或终止
    /// 与 try_connect_attempt 的区别在于 config 由调用方（ProvideCredential）注入凭据
    fn reconnect_connect(&mut self, attempt: u32, config: ConnectionConfig) {
        match self.connect_inner(config, self.known_hosts.clone()) {
            Ok(()) => {
                self.reconnect = ReconnectState::Idle;
                self.dispatch(CoreEvent::Session(SessionEvent::Reconnected {
                    session_id: self.session_id(),
                }));
            }
            Err(e) => {
                tracing::warn!(session_id = ?self.session_id(), attempt, error = %e, "reconnect attempt failed");
                let Some(policy) = self.reconnect_policy.clone() else {
                    self.reconnect = ReconnectState::Idle;
                    return;
                };
                let next = attempt + 1;
                match reconnect_delay(next, &policy) {
                    Some(secs) => {
                        self.reconnect = ReconnectState::Backoff {
                            attempt: next,
                            wake_at: Instant::now() + Duration::from_secs(secs),
                        };
                    }
                    None => {
                        self.reconnect = ReconnectState::Idle;
                        self.dispatch_reconnect_failed("max retries exhausted".into());
                    }
                }
            }
        }
    }

    /// 广播重连失败事件（重试耗尽或无配置骨架）
    fn dispatch_reconnect_failed(&self, reason: String) {
        self.dispatch(CoreEvent::Session(SessionEvent::ReconnectFailed {
            session_id: self.session_id(),
            reason,
        }));
    }

    /// 执行远程命令并返回 stdout（worker 内执行）
    fn exec_inner(&self, command: &str) -> CoreResult<String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::Internal("no active connection".into()))?;
        conn.exec_command(command)
    }

    /// 探测并缓存哈希命令（会话级：同一次连接不会换操作系统）
    fn ensure_hash_cmd(&mut self) -> CoreResult<Option<HashCommand>> {
        if let Some(cmd) = self.hash_cmd {
            return Ok(Some(cmd));
        }
        let present = |name: &str| -> bool {
            // command -v 是 POSIX 内置：存在则 stdout 非空
            self.exec_inner(&format!("command -v {name}"))
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        let selected = select_hash_command(present);
        if let Some(cmd) = selected {
            self.hash_cmd = Some(cmd);
            tracing::info!(?cmd, "remote hash command selected");
        } else {
            tracing::warn!("no hash command available on remote host, checksum will be skipped");
        }
        Ok(selected)
    }

    /// 计算远程文件哈希；远端无可用命令或 exec 失败返回 Ok(None)（跳过校验）
    /// 错误分级（权威设计 §7.3）：exec 错误（连接断开等）也跳过而非报错，仅 warn
    fn remote_sha256_inner(&mut self, path: &str) -> CoreResult<Option<String>> {
        let cmd = match self.ensure_hash_cmd()? {
            Some(c) => c,
            None => return Ok(None),
        };
        let escaped = path.replace('\'', "'\\''");
        let command = match cmd {
            HashCommand::Sha256sum => format!("sha256sum -- '{}'", escaped),
            HashCommand::Shasum => format!("shasum -a 256 -- '{}'", escaped),
            HashCommand::BsdSha256 => format!("sha256 '{}'", escaped),
            HashCommand::Openssl => format!("openssl dgst -sha256 '{}'", escaped),
        };
        let out = match self.exec_inner(&command) {
            Ok(out) => out,
            Err(e) => {
                tracing::warn!(error = %e, "remote hash exec failed, checksum skipped");
                return Ok(None);
            }
        };
        Ok(parse_hash_output(&cmd, &out))
    }

    /// 获取当前 worker 绑定的 session_id（构造时注入，始终有值）
    fn session_id(&self) -> SessionId {
        self.session_id
            .expect("session_id always set at construction")
    }

    /// 打开通道（shell 或 sftp），返回 ChannelId
    fn open_channel_inner(&mut self, ctype: ChannelType) -> CoreResult<ChannelId> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::Internal("no active connection".into()))?;
        let ssh_session = conn
            .session()
            .ok_or_else(|| CoreError::Internal("ssh session not available".into()))?;
        let channel_id = ChannelId::new();
        let sid = self.session_id();
        let dispatcher = self.dispatcher.clone();
        let inner = match ctype {
            ChannelType::Shell => {
                let ch = channel::open_shell_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Session(ch)
            }
            ChannelType::Sftp => {
                let sftp =
                    channel::open_sftp_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Sftp(sftp)
            }
        };
        self.raw_channels.insert(channel_id, inner);
        self.channels.push(channel_id);
        self.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: sid,
            channel_id,
        }));
        Ok(channel_id)
    }

    /// 向通道写入数据（处理部分写入；EAGAIN 重试统一走 io_retry）
    fn channel_write_inner(&mut self, channel: ChannelId, data: &[u8]) -> CoreResult<()> {
        let inner = self
            .raw_channels
            .get_mut(&channel)
            .ok_or_else(|| CoreError::Internal("channel not found".into()))?;
        match inner {
            ChannelInner::Session(ch) => {
                let mut written = 0;
                while written < data.len() {
                    match io_retry(|| ch.write(&data[written..]), &self.cancel) {
                        Ok(n) => written += n,
                        Err(e) => {
                            return Err(CoreError::Internal(format!("channel write failed: {e}")));
                        }
                    }
                }
                Ok(())
            }
            ChannelInner::Sftp(_) => Err(CoreError::Internal(
                "write not supported on sftp channel".into(),
            )),
        }
    }

    /// 调整 PTY 尺寸（EAGAIN 重试统一走 io_retry）
    fn channel_resize_inner(&mut self, channel: ChannelId, cols: u32, rows: u32) -> CoreResult<()> {
        let inner = self
            .raw_channels
            .get_mut(&channel)
            .ok_or_else(|| CoreError::Internal("channel not found".into()))?;
        match inner {
            ChannelInner::Session(ch) => {
                io_retry(
                    || ch.request_pty_size(cols, rows, Some(cols * 8), Some(rows * 16)),
                    &self.cancel,
                )
                .map_err(|e| CoreError::Internal(format!("resize pty failed: {e}")))?;
                Ok(())
            }
            ChannelInner::Sftp(_) => Err(CoreError::Internal(
                "resize not supported on sftp channel".into(),
            )),
        }
    }

    /// 关闭通道（清理 ssh2 资源并广播 Closed 事件）
    fn channel_close_inner(&mut self, channel: ChannelId) -> CoreResult<()> {
        if let Some(inner) = self.raw_channels.remove(&channel) {
            match inner {
                ChannelInner::Session(mut ch) => {
                    let _ = ch.close();
                    let _ = ch.wait_close();
                }
                ChannelInner::Sftp(_) => {}
            }
        }
        self.channels.retain(|c| *c != channel);
        self.dispatch(CoreEvent::Channel(ChannelEvent::Closed {
            session_id: self.session_id(),
            channel_id: channel,
        }));
        Ok(())
    }

    /// 关闭全部通道
    fn close_all_channels_inner(&mut self) {
        let ids: Vec<ChannelId> = self.channels.to_vec();
        for cid in ids {
            let _ = self.channel_close_inner(cid);
        }
    }

    /// 找到 Sftp 通道的原始句柄（每个 session 仅一个 sftp 通道，取第一个）
    fn get_sftp(&mut self) -> CoreResult<&mut ssh2::Sftp> {
        for inner in self.raw_channels.values_mut() {
            if let ChannelInner::Sftp(sftp) = inner {
                return Ok(sftp);
            }
        }
        Err(CoreError::Internal("sftp channel not found".into()))
    }

    /// 当前 Sftp 通道 id（与 get_sftp 同源，用于传输事件广播）
    fn sftp_channel_id(&self) -> Option<ChannelId> {
        self.raw_channels
            .iter()
            .find_map(|(id, inner)| matches!(inner, ChannelInner::Sftp(_)).then_some(*id))
    }

    /// 列出远程目录（过滤 . ..、构建绝对路径、目录优先排序）
    fn sftp_read_dir_inner(&mut self, path: &str) -> CoreResult<Vec<FileEntry>> {
        let entries = sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "readdir",
            |sftp| sftp.readdir(std::path::Path::new(path)),
        )?;
        // 构建绝对路径
        let base = path.trim_end_matches('/');
        let mut files = Vec::new();
        for (p, stat) in entries {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            // 过滤特殊条目与无法表示文件名的条目
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            files.push(FileEntry {
                name: name.clone(),
                path: format!("{}/{}", base, name),
                size: stat.size.unwrap_or(0),
                is_dir: stat.is_dir(),
                modified: format!("{}", stat.mtime.unwrap_or(0)),
            });
        }
        files.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files)
    }

    /// 创建远程目录（权限 0o755）
    fn sftp_create_dir_inner(&mut self, path: &str) -> CoreResult<()> {
        sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "mkdir",
            |sftp| sftp.mkdir(std::path::Path::new(path), 0o755),
        )?;
        Ok(())
    }

    /// 删除远程文件或目录（unlink / rmdir）
    fn sftp_remove_inner(&mut self, path: &str, is_dir: bool) -> CoreResult<()> {
        if is_dir {
            sftp_retry_io(
                &mut self.raw_channels,
                &self.cancel,
                "rmdir",
                |sftp| sftp.rmdir(std::path::Path::new(path)),
            )?;
        } else {
            sftp_retry_io(
                &mut self.raw_channels,
                &self.cancel,
                "unlink",
                |sftp| sftp.unlink(std::path::Path::new(path)),
            )?;
        }
        Ok(())
    }

    /// 重命名远程文件或目录
    fn sftp_rename_inner(&mut self, old: &str, new: &str) -> CoreResult<()> {
        let old_path = std::path::Path::new(old);
        let new_path = std::path::Path::new(new);
        sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "rename",
            |sftp| sftp.rename(old_path, new_path, None),
        )?;
        Ok(())
    }

    /// 传输：流式读写 + 每 chunk 后嵌套处理非传输命令
    /// 进入时广播 Transfer Locked，退出（成功/失败/取消）时广播 Unlocked
    fn handle_transfer_inner(
        &mut self,
        kind: TransferKind,
        remote: &str,
        local: &str,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
    ) -> CoreResult<u64> {
        let sftp_channel_id = self
            .sftp_channel_id()
            .ok_or_else(|| CoreError::Internal("sftp channel not found".into()))?;
        self.transferring = true;
        self.dispatch(CoreEvent::Transfer(TransferEvent::Locked {
            session_id: self.session_id(),
            channel_id: sftp_channel_id,
            direction: match kind {
                TransferKind::Download => TransferDirection::Download,
                TransferKind::Upload => TransferDirection::Upload,
            },
        }));
        let result = match kind {
            TransferKind::Download => self.transfer_download_inner(remote, local, &progress),
            TransferKind::Upload => self.transfer_upload_inner(remote, local, &progress),
        };
        self.dispatch(CoreEvent::Transfer(TransferEvent::Unlocked {
            session_id: self.session_id(),
            channel_id: sftp_channel_id,
        }));
        self.transferring = false;
        result
    }

    /// 下载循环：每 chunk 后嵌套处理队列（终端输入/断开等立即响应）
    fn transfer_download_inner(
        &mut self,
        remote_path: &str,
        local_path: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
    ) -> CoreResult<u64> {
        // 获取远程文件大小（用于进度条）
        let total = sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "stat",
            |sftp| sftp.stat(std::path::Path::new(remote_path)),
        )?
        .size
        .unwrap_or(0);
        // 创建本地文件
        let mut local_file = std::fs::File::create(local_path)
            .map_err(|e| CoreError::Internal(format!("create local file failed: {e}")))?;
        // 打开远程文件（File 持有会话引用，不借用 sftp 句柄）
        let mut file = sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "open",
            |sftp| sftp.open(std::path::Path::new(remote_path)),
        )?;
        let mut done: u64 = 0;
        let mut chunk = [0u8; SFTP_CHUNK_SIZE];
        loop {
            // 嵌套处理已到达命令（断开/输入/列目录等立即响应）
            self.drain_nested_commands();
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                // 取消（断开/Close）：显式错误走统一失败清理，避免 total==0 时误报成功
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            // 读块（EAGAIN 重试统一走 io_retry，重试期间检查取消标志）
            let n = io_retry(|| file.read(&mut chunk), &self.cancel)
                .map_err(|e| CoreError::Internal(format!("sftp read failed: {e}")))?;
            if n == 0 {
                break;
            }
            local_file.write_all(&chunk[..n]).map_err(|e| {
                CoreError::Internal(format!("local write failed: {e}"))
            })?;
            done += n as u64;
            let _ = progress.send((done, total));
        }
        // 完整性校验：写入字节数必须与远端文件大小一致
        if total > 0 && done != total {
            return Err(CoreError::Internal(format!(
                "download size mismatch: got {done} bytes, expected {total}"
            )));
        }
        Ok(done)
    }

    /// 上传循环：每 chunk 后嵌套处理队列（断开/输入等立即响应）
    fn transfer_upload_inner(
        &mut self,
        remote_path: &str,
        local_path: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
    ) -> CoreResult<u64> {
        // 获取本地文件大小（用于进度条）
        let total = std::fs::metadata(local_path)
            .map_err(|e| CoreError::Internal(format!("local metadata failed: {e}")))?
            .len();
        // 打开本地文件
        let mut local_file = std::fs::File::open(local_path)
            .map_err(|e| CoreError::Internal(format!("open local file failed: {e}")))?;
        // 创建远程文件
        let mut file = sftp_retry_io(
            &mut self.raw_channels,
            &self.cancel,
            "create",
            |sftp| sftp.create(std::path::Path::new(remote_path)),
        )?;
        let mut done: u64 = 0;
        let mut chunk = [0u8; SFTP_CHUNK_SIZE];
        let result = (|| -> CoreResult<u64> {
            loop {
                // 嵌套处理已到达命令（断开/输入/列目录等立即响应）
                self.drain_nested_commands();
                if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    // 取消（断开/Close）：显式错误走统一失败清理，避免 total==0 时误报成功
                    return Err(CoreError::Internal("transfer cancelled".into()));
                }
                let n = local_file.read(&mut chunk).map_err(|e| {
                    CoreError::Internal(format!("local read failed: {e}"))
                })?;
                if n == 0 {
                    break;
                }
                // 写入完整块（处理 EAGAIN 与部分写入；重试统一走 io_retry）
                let mut off = 0;
                while off < n {
                    match io_retry(|| file.write(&chunk[off..n]), &self.cancel) {
                        Ok(w) => off += w,
                        Err(e) => {
                            return Err(CoreError::Internal(format!("sftp write failed: {e}")));
                        }
                    }
                }
                done += n as u64;
                let _ = progress.send((done, total));
            }
            // 完整性校验：远端文件大小必须与本地一致
            let remote_size = sftp_retry_io(
                &mut self.raw_channels,
                &self.cancel,
                "stat",
                |sftp| sftp.stat(std::path::Path::new(remote_path)),
            )?
            .size
            .unwrap_or(0);
            if remote_size != done {
                return Err(CoreError::Internal(format!(
                    "upload size mismatch: remote has {remote_size} bytes, expected {done}"
                )));
            }
            Ok(done)
        })();
        if result.is_err() {
            // 清理不完整的远端文件（旧实现行为保持：任何失败路径删除半成品）
            if let Ok(sftp) = self.get_sftp() {
                let _ = sftp.unlink(std::path::Path::new(remote_path));
            }
            tracing::warn!(%remote_path, %local_path, "upload failed, partial remote file removed");
        }
        result
    }

    /// 处理队列中所有已到达命令；返回 false 表示应结束线程
    /// run_loop 主循环与传输循环（transfer_*_inner 每 chunk 后）共用：
    /// 传输期间嵌套到达的传输命令由 handle 内 transferring 检查拒绝（返回 busy）
    fn drain_nested_commands(&mut self) -> bool {
        let mut keep_running = true;
        while let Ok(cmd) = self.rx.try_recv() {
            if !self.handle(cmd) {
                keep_running = false;
                break;
            }
        }
        keep_running
    }

    /// 处理单条命令；返回 false 表示应结束线程
    fn handle(&mut self, cmd: WorkerCommand) -> bool {
        match cmd {
            WorkerCommand::Connect {
                config,
                known_hosts,
                reply,
            } => {
                let r = self.connect_inner(config, known_hosts);
                let _ = reply.send(r);
            }
            WorkerCommand::Disconnect => {
                // 手动断开不应继续自动重连（防御性语义）
                self.reconnect = ReconnectState::Idle;
                self.disconnect_inner();
            }
            WorkerCommand::Close => {
                self.disconnect_inner();
                self.closed = true;
                let _ = self.write_state(SessionState::Closed);
                self.dispatch(CoreEvent::Session(SessionEvent::Closed {
                    session_id: self.session_id(),
                }));
                return false; // 结束线程
            }
            WorkerCommand::Exec { command, reply } => {
                let r = self.exec_inner(&command);
                let _ = reply.send(r);
            }
            WorkerCommand::RemoteSha256 { path, reply } => {
                let r = self.remote_sha256_inner(&path);
                let _ = reply.send(r);
            }
            WorkerCommand::OpenChannel { ctype, reply } => {
                let r = self.open_channel_inner(ctype);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelWrite {
                channel,
                data,
                reply,
            } => {
                let r = self.channel_write_inner(channel, &data);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelResize {
                channel,
                cols,
                rows,
                reply,
            } => {
                let r = self.channel_resize_inner(channel, cols, rows);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelClose { channel, reply } => {
                let r = self.channel_close_inner(channel);
                let _ = reply.send(r);
            }
            WorkerCommand::CloseAllChannels => {
                self.close_all_channels_inner();
            }
            WorkerCommand::SftpReadDir { path, reply } => {
                let r = self.sftp_read_dir_inner(&path);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpCreateDir { path, reply } => {
                let r = self.sftp_create_dir_inner(&path);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpRemove {
                path,
                is_dir,
                reply,
            } => {
                let r = self.sftp_remove_inner(&path, is_dir);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpRename { old, new, reply } => {
                let r = self.sftp_rename_inner(&old, &new);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpTransfer {
                kind,
                remote,
                local,
                progress,
                reply,
            } => {
                // 传输命令在传输中再次到达：拒绝（避免嵌套传输）
                if self.transferring {
                    let _ = reply.send(Err(CoreError::Internal(
                        "transfer already in progress".into(),
                    )));
                    return !self.closed;
                }
                let r = self.handle_transfer_inner(kind, &remote, &local, progress);
                let _ = reply.send(r);
            }
            WorkerCommand::ProvideCredential { secret } => {
                if let ReconnectState::AwaitingCredential { attempt, .. } = &self.reconnect {
                    let attempt = *attempt;
                    if let Some(profile) = self.host_config.clone() {
                        // 凭据注入（仅当值被使用；骨架不含凭据值）
                        let mut config = profile_to_config(&profile);
                        config.auth_method = match &profile.auth_kind {
                            ProfileAuthKind::Password => AuthMethod::Password(secret),
                            ProfileAuthKind::PrivateKey { path, .. } => AuthMethod::PrivateKey {
                                path: path.clone(),
                                passphrase: Some(secret),
                            },
                        };
                        self.reconnect_connect(attempt, config);
                    } else {
                        // 配置骨架缺失（未经历过成功连接）：终止等待
                        self.reconnect = ReconnectState::Idle;
                    }
                }
            }
            WorkerCommand::SetReconnectPolicy { policy } => {
                self.reconnect_policy = policy;
            }
        }
        !self.closed
    }

    /// 空闲工作：重连状态机推进、keepalive 发送与 shell 通道非阻塞轮询读
    fn do_idle_work(&mut self) {
        if self.closed {
            return;
        }
        // 重连状态机推进（断开期间工作；连接中 Idle 无操作）
        self.advance_reconnect();
        if self.connection.is_none() {
            return; // 未连接不发送 keepalive
        }
        // keepalive：传输进行中 worker 忙，此函数不会执行，语义天然正确
        if keepalive_due(
            self.last_keepalive,
            std::time::Instant::now(),
            KEEPALIVE_INTERVAL,
        ) {
            let result = self
                .connection
                .as_ref()
                .and_then(|conn| conn.session())
                .map(|s| s.keepalive_send());
            match result {
                Some(Ok(_)) => {
                    self.last_keepalive = Some(std::time::Instant::now());
                }
                Some(Err(e)) => {
                    tracing::warn!(session_id = ?self.session_id(), error = %e, "keepalive failed, disconnecting and scheduling reconnect");
                    self.disconnect_inner();
                    self.schedule_reconnect();
                    return;
                }
                None => {}
            }
        }
        // shell 通道轮询读
        let ids: Vec<ChannelId> = self.channels.clone();
        for id in ids {
            let _ = self.poll_shell_read(id);
        }
    }

    /// 非阻塞读一次 shell 通道；有数据则 dispatch DataReceived
    fn poll_shell_read(&mut self, channel: ChannelId) -> CoreResult<()> {
        let inner = match self.raw_channels.get_mut(&channel) {
            Some(i) => i,
            None => return Ok(()), // 已关闭
        };
        let ChannelInner::Session(ch) = inner else {
            return Ok(());
        };
        let mut buf = [0u8; 4096];
        match ch.read(&mut buf) {
            Ok(0) | Err(_) => Ok(()), // EAGAIN 或无数据：无事
            Ok(n) => {
                let data = Vec::from(&buf[..n]);
                self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                    session_id: self.session_id(),
                    channel_id: channel,
                    data,
                }));
                Ok(())
            }
        }
    }
}

/// 统一的 EAGAIN 重试：指数退避 2→32ms 封顶；每轮迭代检查取消标志
/// 错误判定用 is_would_block（错误码匹配 + source 链查找，兜底字符串匹配），
/// 兼容 ssh2::Error 与包装它的 io::Error（io::Read/io::Write trait 返回 io::Error）
/// 仅在 worker 线程内调用（sleep 重试不阻塞其他逻辑）
/// 累计重试时长约 1s 封顶（与旧 sftp_retry 20×50ms 语义一致）：达到上限返回当前
/// EAGAIN 错误，走调用方统一失败清理路径，避免远端无响应时无限重试
fn io_retry<T, E>(
    mut op: impl FnMut() -> Result<T, E>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<T, E>
where
    E: std::error::Error + 'static,
{
    const RETRY_CAP: Duration = Duration::from_millis(1000);
    let start = Instant::now();
    let mut delay = 2u64;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if crate::ssh::is_would_block(&e)
                    && !cancel.load(std::sync::atomic::Ordering::Relaxed)
                    && start.elapsed() < RETRY_CAP
                {
                    std::thread::sleep(Duration::from_millis(delay));
                    delay = (delay * 2).min(32);
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// 对 sftp 通道执行一次操作并统一 EAGAIN 重试（io_retry）
/// 同时借用 raw_channels 与 cancel 两个字段（字段级借用，互不冲突），
/// 避免经 get_sftp 的整体 &mut self 借用与取消标志检查发生冲突
fn sftp_retry_io<T, F>(
    channels: &mut HashMap<ChannelId, ChannelInner>,
    cancel: &AtomicBool,
    what: &str,
    mut op: F,
) -> CoreResult<T>
where
    F: FnMut(&mut ssh2::Sftp) -> Result<T, ssh2::Error>,
{
    let sftp = channels
        .values_mut()
        .find_map(|inner| match inner {
            ChannelInner::Sftp(s) => Some(s),
            _ => None,
        })
        .ok_or_else(|| CoreError::Internal("sftp channel not found".into()))?;
    io_retry(|| op(sftp), cancel)
        .map_err(|e| CoreError::Internal(format!("sftp {what} failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_common::{ConnectionConfig, SessionId, SessionState};
    use core_event::LoggingDispatcher;

    fn spawn_test_worker() -> (Arc<WorkerHandle>, Arc<std::sync::RwLock<SessionState>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(std::sync::RwLock::new(SessionState::Created));
        let state_clone = state.clone();
        let dispatcher: Arc<dyn EventDispatcher> = Arc::new(LoggingDispatcher);
        let session_id = SessionId::new();
        let join = std::thread::spawn(move || run_loop(rx, state_clone, dispatcher, session_id));
        let handle = Arc::new(WorkerHandle::new(tx));
        handle.set_join(join);
        (handle, state)
    }

    #[test]
    fn test_worker_alive_after_spawn() {
        let (handle, _) = spawn_test_worker();
        // 未连接时开启通道必须回执"no active connection"错误（不可挂起）
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle
            .send(WorkerCommand::OpenChannel {
                ctype: core_common::ChannelType::Shell,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx
            .blocking_recv()
            .expect("worker 必须回执，不允许挂起");
        assert!(result.is_err(), "未连接时打开通道应返回错误");
    }

    #[test]
    fn test_open_channel_without_connection_fails() {
        let (handle, _) = spawn_test_worker();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle
            .send(WorkerCommand::OpenChannel {
                ctype: core_common::ChannelType::Shell,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx.blocking_recv().unwrap();
        let err = result.expect_err("未连接时打开通道应返回错误");
        assert!(
            err.to_string().contains("no active connection"),
            "错误消息应明确说明无连接，而非骨架阶段的 not implemented 兜底。实际消息: {}",
            err,
        );
    }

    #[test]
    fn test_sftp_read_dir_without_connection_fails() {
        let (handle, _) = spawn_test_worker();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle
            .send(WorkerCommand::SftpReadDir {
                path: "/".into(),
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx.blocking_recv().unwrap();
        let err = result.expect_err("未连接时 SFTP 命令应返回错误");
        assert!(
            err.to_string().contains("sftp channel not found")
                || err.to_string().contains("no active connection"),
            "错误消息应说明连接/通道缺失，而非骨架阶段的 not implemented 兜底。实际消息: {err}",
        );
    }

    #[test]
    fn test_close_terminates_worker() {
        let (handle, _) = spawn_test_worker();
        handle.send(WorkerCommand::Close).unwrap();
        // 关闭命令后 worker 应退出（join 可完成）
        let join = handle.join.lock().unwrap().take().unwrap();
        join.join().expect("worker thread should exit after Close");
    }

    #[test]
    fn test_connect_rejects_invalid_host() {
        let (handle, state) = spawn_test_worker();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let config = ConnectionConfig::new(
            "nonexistent.invalid".into(),
            "root".into(),
            core_common::AuthMethod::Password("x".into()),
        )
        .with_timeout(1); // 1 秒超时，测试不挂起
        handle
            .send(WorkerCommand::Connect {
                config,
                known_hosts: None,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx.blocking_recv().unwrap();
        assert!(result.is_err(), "连接无效主机应返回错误");
        assert_eq!(*state.read().unwrap(), SessionState::Disconnected);
    }

    #[test]
    fn test_reconnect_delay_sequence() {
        let p = core_common::ReconnectPolicy {
            max_retries: 3,
            base_delay_secs: 1,
            max_delay_secs: 30,
        };
        assert_eq!(reconnect_delay(1, &p), Some(1));
        assert_eq!(reconnect_delay(2, &p), Some(2));
        assert_eq!(reconnect_delay(3, &p), Some(4));
        assert_eq!(reconnect_delay(4, &p), None); // 超出 max_retries
    }

    #[test]
    fn test_keepalive_due() {
        let start = std::time::Instant::now();
        assert!(!keepalive_due(
            None,
            start,
            std::time::Duration::from_secs(30)
        ));
        assert!(keepalive_due(
            Some(start - std::time::Duration::from_secs(31)),
            std::time::Instant::now(),
            std::time::Duration::from_secs(30)
        ));
        assert!(!keepalive_due(
            Some(start - std::time::Duration::from_secs(29)),
            std::time::Instant::now(),
            std::time::Duration::from_secs(30)
        ));
    }

    #[test]
    fn test_parse_hash_output_sha256sum() {
        assert_eq!(
            parse_hash_output(&HashCommand::Sha256sum, "abc123  /etc/file").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn test_parse_hash_output_openssl() {
        assert_eq!(
            parse_hash_output(&HashCommand::Openssl, "SHA2-256(/etc/file)= abc123").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn test_parse_hash_output_bsd() {
        assert_eq!(
            parse_hash_output(&HashCommand::BsdSha256, "SHA256 (/etc/file) = abc123").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn test_parse_hash_output_empty() {
        assert_eq!(parse_hash_output(&HashCommand::Sha256sum, ""), None);
    }

    #[test]
    fn test_select_hash_command_prefers_sha256sum() {
        // 探测结果决策：优先 sha256sum
        let present = |name: &str| -> bool { name == "sha256sum" };
        let selected = select_hash_command(present);
        assert!(matches!(selected, Some(HashCommand::Sha256sum)));
    }

    #[test]
    fn test_select_hash_command_none_available() {
        let present = |_: &str| -> bool { false };
        assert!(select_hash_command(present).is_none());
    }

    #[test]
    fn test_io_retry_retries_would_block_then_succeeds() {
        // EAGAIN 两次后成功：io_retry 应退避重试直至成功
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let r = io_retry(
            || {
                calls += 1;
                if calls < 3 {
                    Err(ssh2::Error::new(
                        ssh2::ErrorCode::Session(-37),
                        "Would block waiting for status message",
                    ))
                } else {
                    Ok(42u64)
                }
            },
            &cancel,
        );
        assert_eq!(r.unwrap(), 42);
        assert_eq!(calls, 3);
    }

    #[test]
    fn test_io_retry_cancel_stops_retrying() {
        // 取消标志置位：EAGAIN 不得继续重试，立即返回错误
        let cancel = AtomicBool::new(true);
        let mut calls = 0;
        let r: Result<u64, ssh2::Error> = io_retry(
            || {
                calls += 1;
                Err(ssh2::Error::new(
                    ssh2::ErrorCode::Session(-37),
                    "would block",
                ))
            },
            &cancel,
        );
        assert!(r.is_err());
        assert_eq!(calls, 1, "cancel 置位时不得重试");
    }

    #[test]
    fn test_io_retry_cap_returns_error() {
        // 持续 EAGAIN 且无取消：累计约 1s 后必须返回错误，不得无限重试
        let cancel = AtomicBool::new(false);
        let start = std::time::Instant::now();
        let r: Result<u64, ssh2::Error> = io_retry(
            || {
                Err(ssh2::Error::new(
                    ssh2::ErrorCode::Session(-37),
                    "Would block waiting for status message",
                ))
            },
            &cancel,
        );
        assert!(r.is_err());
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(950),
            "应重试至累计约 1s 上限，实际 {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(2500),
            "不应超过上限过久，实际 {elapsed:?}"
        );
    }
}
