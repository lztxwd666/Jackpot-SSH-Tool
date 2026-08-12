//! SSH 执行线程模块
//! 每 Session 一个 worker 线程，串行执行全部 ssh2 操作（Active Object 模式）
//! 事件循环: try_recv 处理命令 → 空闲工作（keepalive / shell 读轮询）→ sleep 25ms
//! 拆分为同模块多文件（方法跨文件可见，无模块级循环）：sftp.rs 为 sftp 单操作，
//! transfer.rs 为流式/目录传输，本文件为结构、命令循环与连接/通道/idle 工作

mod sftp;
mod transfer;

use crate::ssh::SshConnection;
use crate::ssh::io_retry; // 统一的 EAGAIN 重试原语（定义在 ssh::retry，供全部 ssh2 操作共用）
use core_common::{
    ChannelId, ChannelType, ConnectionConfig, CoreError, CoreResult, FileEntry,
    KnownHostsProvider, PtySize, SessionId, SessionState,
};
use core_event::EventDispatcher;
use core_event::event::{ChannelEvent, CoreEvent, SessionEvent};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tokio::sync::oneshot;

/// worker 空闲工作间隔（毫秒）：shell 读轮询与命令响应延迟的上限
const IDLE_INTERVAL_MS: u64 = 25;

/// keepalive 发送间隔：连接空闲时每 30 秒发送一次 SSH keepalive 请求以保持连接活跃
const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// SFTP 流式传输分块大小（64KB）：减少循环次数加速大文件，兼顾内存占用
const SFTP_CHUNK_SIZE: usize = 65536;

/// keepalive 间隔判定：now 距 last 达到 interval 返回 true
/// None 表示无计时起点（连接建立前）：返回 false 不发送；
/// 连接建立时 last_keepalive 置为连接时刻，30 秒后首次到期（首轮不立即发送）
fn keepalive_due(
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    interval: std::time::Duration,
) -> bool {
    match last {
        None => false, // 无计时起点：未连接状态
        Some(l) => now.duration_since(l) >= interval,
    }
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
/// Openssl/BsdSha256 输出形如 "SHA2-256(/path)= hash"：路径可能含 '=' 或空格，
/// 从尾部提取 64 位十六进制 token 最稳健（哈希值固定格式，路径内容不干扰）
pub(crate) fn parse_hash_output(cmd: &HashCommand, out: &str) -> Option<String> {
    match cmd {
        HashCommand::Sha256sum | HashCommand::Shasum => {
            out.split_whitespace().next().map(|s| s.to_string())
        }
        HashCommand::Openssl | HashCommand::BsdSha256 => out
            .split_whitespace()
            .rev()
            .find(|s| {
                s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .map(|s| s.to_string()),
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
    SftpCreateFile {
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
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
        reply: oneshot::Sender<CoreResult<u64>>,
    },
    /// 目录递归传输（批量文件，聚合进度；第三位为当前文件名）
    SftpTransferTree {
        kind: TransferKind,
        remote: String,
        local: String,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
        reply: oneshot::Sender<CoreResult<u64>>,
    },
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
/// state 与 Session 共享（外部查询状态）
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

/// 内部通道类型（worker 通道注册表条目；归属 worker 而非 channel 模块，
/// 保持 channel → worker 单向依赖，避免模块级循环依赖）
enum ChannelInner {
    // stderr_eof：通道 stderr 流已结束（motd 等 PAM 横幅经 sshd 发到 stderr；
    // stderr 的 EOF 不代表通道关闭，仅停止该流读取）
    Session {
        ch: ssh2::Channel,
        stderr_eof: bool,
    },
    Sftp(ssh2::Sftp),
}

/// PTY 终端模式 opcode 常量（RFC 4254 §8 定义；ICANON=51、ISIG=50，非 2/3）
const ECHO: u8 = 53;
const ICANON: u8 = 51;
const ISIG: u8 = 50;
const ICRNL: u8 = 36;
const ONLCR: u8 = 72;
const OPOST: u8 = 70;

/// worker 内部状态：全部 ssh2 相关状态移入此处（无锁，单线程独占）
struct Worker {
    state: Arc<std::sync::RwLock<SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    closed: bool,
    session_id: SessionId,             // run_loop 启动时注入（构造必有值，直接持有）
    connection: Option<SshConnection>, // ssh2 连接所有权（Task 2 引入）
    last_keepalive: Option<Instant>,   // Task 4 使用
    cancel: AtomicBool,                // Task 5 传输取消使用
    raw_channels: HashMap<ChannelId, ChannelInner>, // worker 内通道注册表
    channels: Vec<ChannelId>,          // 已打开通道 ID 列表
    rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>, // 命令队列（嵌套 try_recv 取命令）
    transferring: bool,                // 传输进行中：拒绝嵌套传输
    hash_cmd: Option<HashCommand>,     // 远端哈希命令探测结果缓存（会话级）
    // 传输中到达的断开/关闭（延迟处理）：cancel 置位立即中止传输，但连接释放必须等
    // 传输栈弹出（其局部 ssh2::File drop 会解引用 session，提前释放即 use-after-free）
    pending_disconnect: Option<String>,
    pending_close: bool,
    // USERAUTH_BANNER 已注入标志：认证横幅在认证期间到达（RFC 4252 §5.4，
    // libssh2 经 userauth_banner 读取，区别于 session.banner 的版本标识），
    // 终端此时尚未创建，于首个 shell 通道打开时注入；多次 open_shell 不得重复注入
    banner_injected: bool,
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
            session_id,
            connection: None,
            last_keepalive: None,
            closed: false,
            cancel: AtomicBool::new(false),
            raw_channels: HashMap::new(),
            channels: Vec::new(),
            rx,
            transferring: false,
            hash_cmd: None,
            pending_disconnect: None,
            pending_close: false,
            banner_injected: false,
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
                self.cancel
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                // keepalive 计时起点：以连接建立时刻计，30 秒空闲后到期发送（首轮不立即发送）
                self.last_keepalive = Some(std::time::Instant::now());
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
    /// 断开即断：置位取消标志，传输循环（transfer_*_inner）在下一个 chunk 检查点立即中止
    /// reason：断开原因（异常断开时随 Disconnected 事件广播，供 UI 展示）
    /// 传输进行中调用时延迟释放连接（cancel 置位立即中止传输；连接与通道的释放推迟到
    /// 传输栈弹出后的 flush_pending，否则传输函数内 ssh2::File drop 会解引用已释放的
    /// session，use-after-free）
    fn disconnect_inner(&mut self, reason: &str) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if self.transferring {
            // 首个断开原因优先：嵌套命令失败（如传输中排队的 ChannelWrite）不得
            // 覆盖已挂起的原因，避免 Disconnected 事件展示误导性文案
            if self.pending_disconnect.is_none() {
                self.pending_disconnect = Some(reason.to_string());
            }
            return;
        }
        self.close_all_channels_inner();
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.disconnect();
        }
        if self.write_state(SessionState::Disconnected).is_ok() {
            self.dispatch(CoreEvent::Session(SessionEvent::Disconnected {
                session_id: self.session_id(),
                reason: reason.to_string(),
            }));
        }
    }

    /// 传输结束后冲刷挂起的断开/关闭（传输期间到达的 Disconnect/Close 延迟处理）
    /// 必须在传输函数返回、其局部 ssh2 句柄已 drop 后调用；Close 优先于 Disconnect
    /// （关闭即断开），并保持 Disconnected → Closed 的事件顺序
    fn flush_pending(&mut self) {
        if self.pending_close {
            self.pending_close = false;
            self.disconnect_inner("session closed");
            self.closed = true;
            let _ = self.write_state(SessionState::Closed);
            self.dispatch(CoreEvent::Session(SessionEvent::Closed {
                session_id: self.session_id(),
            }));
        } else if let Some(reason) = self.pending_disconnect.take() {
            self.disconnect_inner(&reason);
        }
    }

    /// 执行远程命令并返回 stdout（worker 内执行）
    /// 传入取消标志：断开（cancel 置位）后 exec 各阶段立即失败，避免死连接上无限重试
    fn exec_inner(&self, command: &str) -> CoreResult<String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::Internal("no active connection".into()))?;
        conn.exec_command(command, &self.cancel)
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
        // -- 结束选项解析：以 - 开头的路径不会被当作选项（sha256sum/shasum 已带，补全 bsd/openssl）
        let command = match cmd {
            HashCommand::Sha256sum => format!("sha256sum -- '{}'", escaped),
            HashCommand::Shasum => format!("shasum -a 256 -- '{}'", escaped),
            HashCommand::BsdSha256 => format!("sha256 -- '{}'", escaped),
            HashCommand::Openssl => format!("openssl dgst -sha256 -- '{}'", escaped),
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

    /// 获取当前 worker 绑定的 session_id（构造时注入）
    fn session_id(&self) -> SessionId {
        self.session_id
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
                let ch = open_shell_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Session { ch, stderr_eof: false }
            }
            ChannelType::Sftp => {
                let sftp = open_sftp_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Sftp(sftp)
            }
        };
        self.raw_channels.insert(channel_id, inner);
        self.channels.push(channel_id);

        // USERAUTH_BANNER（RFC 4252 §5.4）：服务器认证期间发送的横幅（欢迎/法律声明），
        // 客户端应显示；终端在连接后才创建，认证时无显示载体，故于首个 shell 通道打开时
        // 注入为该通道首批数据（显示于终端顶部、motd 之前）。注意用 userauth_banner
        // 而非 session.banner——后者是协议版本标识（SSH-2.0-...），不是认证横幅
        if ctype == ChannelType::Shell && !self.banner_injected {
            let banner = self
                .connection
                .as_ref()
                .and_then(|c| c.session())
                .and_then(|s| s.userauth_banner().ok().flatten())
                .filter(|b| !b.is_empty());
            if let Some(banner) = banner {
                self.banner_injected = true;
                let mut data = banner.as_bytes().to_vec();
                if !data.ends_with(b"\n") {
                    data.push(b'\n'); // 横幅文本可能不带尾换行，补一行保证显示完整
                }
                self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                    session_id: sid,
                    channel_id,
                    data,
                }));
            }
        }

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
            ChannelInner::Session { ch, .. } => {
                if let Err(e) = write_full(&self.cancel, |buf| ch.write(buf), data) {
                    // 写失败（连接已死或已断开）：主动断开并广播（原因供 UI 展示）
                    let msg = format!("channel write failed: {e}");
                    self.disconnect_inner(&msg);
                    return Err(CoreError::Internal(msg));
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
            ChannelInner::Session { ch, .. } => {
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
    /// 通道不存在时静默返回（重复关闭/已清理的场景不重复广播 Closed 事件）
    fn channel_close_inner(&mut self, channel: ChannelId) -> CoreResult<()> {
        if let Some(inner) = self.raw_channels.remove(&channel) {
            match inner {
                ChannelInner::Session { mut ch, .. } => {
                    let _ = ch.close();
                    let _ = ch.wait_close();
                }
                ChannelInner::Sftp(_) => {}
            }
            self.channels.retain(|c| *c != channel);
            self.dispatch(CoreEvent::Channel(ChannelEvent::Closed {
                session_id: self.session_id(),
                channel_id: channel,
            }));
        }
        Ok(())
    }

    /// 关闭全部通道
    fn close_all_channels_inner(&mut self) {
        let ids: Vec<ChannelId> = self.channels.to_vec();
        for cid in ids {
            let _ = self.channel_close_inner(cid);
        }
    }

    /// 处理队列中所有已到达命令；返回 false 表示应结束线程
    /// run_loop 主循环与传输循环（transfer_*_inner 每 chunk 后）共用：
    /// 传输期间嵌套到达的传输命令由 handle 内 transferring 检查拒绝（返回 busy）
    fn drain_nested_commands(&mut self) -> bool {
        let mut keep_running = true;
        loop {
            match self.rx.try_recv() {
                Ok(cmd) => {
                    if !self.handle(cmd) {
                        keep_running = false;
                        break;
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break, // 队列已清空
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    // 所有 Sender 已丢弃（Session 被废弃且 Drop 未投递 Close 等场景）：
                    // 结束线程，防止 worker 线程泄漏
                    keep_running = false;
                    break;
                }
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
                self.disconnect_inner("disconnected by user");
            }
            WorkerCommand::Close => {
                if self.transferring {
                    // 传输进行中：置位取消标志立即中止传输（断开即断，传输循环在
                    // 下一个 chunk 检查点返回取消错误，不完整文件由
                    // Channel::sftp_download_file 错误路径清理）；连接释放延迟到
                    // 传输栈弹出（flush_pending，防 use-after-free）
                    self.cancel
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    self.pending_close = true;
                } else {
                    self.disconnect_inner("session closed");
                    self.closed = true;
                    let _ = self.write_state(SessionState::Closed);
                    self.dispatch(CoreEvent::Session(SessionEvent::Closed {
                        session_id: self.session_id(),
                    }));
                    return false; // 结束线程
                }
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
            WorkerCommand::SftpReadDir { path, reply } => {
                let r = self.sftp_read_dir_inner(&path);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpCreateDir { path, reply } => {
                let r = self.sftp_create_dir_inner(&path);
                let _ = reply.send(r);
            }
            WorkerCommand::SftpCreateFile { path, reply } => {
                let r = self.sftp_create_file_inner(&path);
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
            } => self.run_transfer_cmd(
                reply,
                move |w| w.handle_transfer_inner(kind, &remote, &local, progress),
            ),
            WorkerCommand::SftpTransferTree {
                kind,
                remote,
                local,
                progress,
                reply,
            } => self.run_transfer_cmd(
                reply,
                move |w| w.handle_transfer_tree_inner(kind, &remote, &local, progress),
            ),
        }
        !self.closed
    }

    /// 空闲工作：keepalive 发送与 shell 通道非阻塞轮询读
    fn do_idle_work(&mut self) {
        if self.closed {
            return;
        }
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
                    tracing::warn!(session_id = ?self.session_id(), error = %e, "keepalive failed, disconnecting");
                    self.disconnect_inner(&format!("keepalive failed: {e}"));
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

    /// 非阻塞读 shell 通道（stdout 排空 + stderr 一次读取）；有数据则 dispatch DataReceived
    fn poll_shell_read(&mut self, channel: ChannelId) -> CoreResult<()> {
        // stdout 读取（块作用域收束通道借用：EOF/读错误断开后不再继续）
        {
            let inner = match self.raw_channels.get_mut(&channel) {
                Some(i) => i,
                None => return Ok(()), // 已关闭
            };
            let ChannelInner::Session { ch, .. } = inner else {
                return Ok(());
            };
            // 循环读到 EAGAIN 一次性排空可达数据：此前每轮只读一次（≤4096B），高吞吐输出
            // （日志流/大文件 cat）被压制在约 160KB/s 且事件过密，前端消费不及时会触发
            // broadcast Lagged 丢弃（终端内容确实丢失）。聚合为单次 DataReceived 事件后
            // 事件数降至 1/16，Lagged 概率随之骤降。每轮累计上限 64KB：
            // 避免单轮无限读取饿死 idle（keepalive/命令响应延迟）
            const SHELL_READ_BUDGET: usize = 64 * 1024;
            let mut data = Vec::new();
            loop {
                let mut buf = [0u8; 4096];
                match ch.read(&mut buf) {
                    Ok(0) => {
                        // EOF：shell 已由远端退出（exit），会话对用户已结束。转为 Disconnected
                        // 状态：前端标签显示断连遮罩与重连入口（此前 EOF 只关通道，标签停在
                        // connected 且无重连入口，用户只能关标签重开）。循环内已读到的尾部
                        // 数据先派发，不丢最后输出；断开流程统一清理全部通道并广播 Closed
                        if !data.is_empty() {
                            self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                                session_id: self.session_id(),
                                channel_id: channel,
                                data,
                            }));
                        }
                        let msg = "session ended (remote exited)".to_string();
                        self.disconnect_inner(&msg);
                        return Err(CoreError::Internal(msg));
                    }
                    Ok(n) => {
                        data.extend_from_slice(&buf[..n]);
                        if data.len() >= SHELL_READ_BUDGET {
                            break;
                        }
                    }
                    Err(e) => {
                        if !crate::ssh::is_would_block(&e) {
                            // 真实读错误：连接已死，主动断开并广播（原因供 UI 展示）
                            let msg = format!("channel read failed: {e}");
                            tracing::warn!(session_id = ?self.session_id(), channel_id = ?channel, error = %e, "shell read failed, disconnecting");
                            self.disconnect_inner(&msg);
                            return Err(CoreError::Internal(msg));
                        }
                        // EAGAIN：stdout 已排空，落到 stderr 读取（motd 等 PAM 横幅经
                        // stderr 到达，此前只在 stdout 同轮有数据时才会被读到）
                        break;
                    }
                }
            }
            if !data.is_empty() {
                self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                    session_id: self.session_id(),
                    channel_id: channel,
                    data,
                }));
            }
        }
        // stderr 流读取：部分程序（TUI 报错、脚本提示等）将输出写入通道 stderr，
        // 真实终端将 stderr 与 stdout 同屏显示；此前只读 stdout 导致 stderr 数据被丢弃。
        // stderr 的 EOF 不代表通道关闭（stdout 可能仍活跃），仅置标志停止本流读取。
        // 重新取通道（与 stdout 分开作用域，避免 dispatch 与通道可变借用互斥）
        let inner = match self.raw_channels.get_mut(&channel) {
            Some(i) => i,
            None => return Ok(()),
        };
        let ChannelInner::Session { ch, stderr_eof } = inner else {
            return Ok(());
        };
        if !*stderr_eof {
            let mut ebuf = [0u8; 4096];
            match ch.stderr().read(&mut ebuf) {
                Ok(0) => *stderr_eof = true,
                Ok(n) => {
                    let data = Vec::from(&ebuf[..n]);
                    self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                        session_id: self.session_id(),
                        channel_id: channel,
                        data,
                    }));
                }
                Err(e) => {
                    if crate::ssh::is_would_block(&e) {
                        return Ok(()); // EAGAIN：无事
                    }
                    let msg = format!("channel stderr read failed: {e}");
                    tracing::warn!(session_id = ?self.session_id(), channel_id = ?channel, error = %e, "shell stderr read failed, disconnecting");
                    self.disconnect_inner(&msg);
                    return Err(CoreError::Internal(msg));
                }
            }
        }
        Ok(())
    }
}

/// 完整写入（通道写入与上传传输共用）：处理部分写入与零进度
/// libssh2 写窗口满时返回 Ok(0)（非 EAGAIN，io_retry 对 Ok 无退避），不加防护会
/// 无限忙循环且不可中断；零进度短退避后重试，取消标志（断开）置位立即中止
fn write_full<W>(cancel: &AtomicBool, mut write: W, data: &[u8]) -> CoreResult<()>
where
    // ssh2 的 Channel/File 实现 std::io::Write（返回 io::Error；is_would_block 已兼容）
    W: FnMut(&[u8]) -> Result<usize, std::io::Error>,
{
    let mut written = 0;
    while written < data.len() {
        match io_retry(|| write(&data[written..]), cancel) {
            Ok(0) => {
                // 零进度：写窗口满，短退避后重试；cancel 预置位（断开已生效）时中止
                // （循环内不处理命令队列，断开命令在传输循环检查点生效后置位）
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(CoreError::Internal("write cancelled".into()));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(n) => written += n,
            Err(e) => {
                return Err(CoreError::Internal(format!("ssh2 write failed: {e}")));
            }
        }
    }
    Ok(())
}

/// 创建 Shell 通道的原始 ssh2::Channel（worker 内调用，不注册到外部队列）
fn open_shell_raw(
    ssh_session: &ssh2::Session,
    session_id: SessionId,
    channel_id: ChannelId,
    dispatcher: Arc<dyn EventDispatcher>,
) -> CoreResult<ssh2::Channel> {
    dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
        session_id,
        channel_id,
        channel_type: ChannelType::Shell,
    }));

    let mut inner_ch = ssh_session.channel_session().map_err(|e| {
        CoreError::Internal(format!("open channel session failed: {e}"))
    })?;

    // 设置标准 PTY 终端模式（与 OpenSSH 行为对齐）
    let mut modes = ssh2::PtyModes::new();
    modes.set_boolean(ECHO, true);
    modes.set_boolean(ICANON, true);
    modes.set_boolean(ISIG, true);
    modes.set_boolean(ICRNL, true);
    modes.set_boolean(ONLCR, true);
    modes.set_boolean(OPOST, true);

    let pty = PtySize::default();
    inner_ch
        .request_pty(
            "xterm-256color",
            Some(modes),
            Some((pty.cols, pty.rows, pty.width_px, pty.height_px)),
        )
        .map_err(|e| CoreError::Internal(format!("request pty failed: {e}")))?;

    inner_ch
        .shell()
        .map_err(|e| CoreError::Internal(format!("start shell failed: {e}")))?;

    // 切换到非阻塞模式
    ssh_session.set_blocking(false);

    tracing::info!(%channel_id, %session_id, "shell channel opened (nonblocking)");
    Ok(inner_ch)
}

/// 创建 SFTP 通道的原始 ssh2::Sftp（worker 内调用，不注册到外部队列）
fn open_sftp_raw(
    ssh_session: &ssh2::Session,
    session_id: SessionId,
    channel_id: ChannelId,
    dispatcher: Arc<dyn EventDispatcher>,
) -> CoreResult<ssh2::Sftp> {
    dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
        session_id,
        channel_id,
        channel_type: ChannelType::Sftp,
    }));

    // SFTP 初始化需要在阻塞模式下完成；失败时同样恢复非阻塞
    // （阻塞模式遗留会使 io_retry 的非阻塞假设失效，worker 可能被单次操作卡住）
    ssh_session.set_blocking(true);
    let result = ssh_session.sftp();
    ssh_session.set_blocking(false);
    let sftp = result.map_err(|e| CoreError::Internal(format!("open sftp failed: {e}")))?;

    tracing::info!(%channel_id, %session_id, "sftp channel opened");
    Ok(sftp)
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
    fn test_worker_exits_when_all_senders_dropped() {
        // 所有 Sender 丢弃（无 Close 命令）：drain 的 Disconnected 分支必须结束线程，
        // 防止废弃 Session 泄漏 worker 线程
        let (handle, _) = spawn_test_worker();
        let join = handle.join.lock().unwrap().take().unwrap();
        drop(handle); // 丢弃唯一 Sender（WorkerHandle 持 tx）
        join.join()
            .expect("worker thread should exit when all senders dropped");
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
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        // 路径含 '=' 与空格：新逻辑从尾部提取 64 位 hex，不受路径内容干扰
        assert_eq!(
            parse_hash_output(&HashCommand::Openssl, &format!("SHA2-256(/etc/a=b c)= {hash}")).as_deref(),
            Some(hash)
        );
    }

    #[test]
    fn test_parse_hash_output_bsd() {
        let hash = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        assert_eq!(
            parse_hash_output(&HashCommand::BsdSha256, &format!("SHA256 (/etc/file) = {hash}")).as_deref(),
            Some(hash)
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
}
