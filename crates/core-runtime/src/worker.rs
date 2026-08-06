//! SSH 执行线程模块
//! 每 Session 一个 worker 线程，串行执行全部 ssh2 操作（Active Object 模式）
//! 事件循环: try_recv 处理命令 → 空闲工作（keepalive / shell 读轮询）→ sleep 25ms

#![allow(dead_code)]

use crate::channel::{self, ChannelInner};
use crate::ssh::SshConnection;
use crate::ssh::io_retry; // 统一的 EAGAIN 重试原语（定义在 ssh::retry，供全部 ssh2 操作共用）
use core_common::{
    ChannelId, ChannelType, ConnectionConfig, CoreError, CoreResult, FileEntry,
    KnownHostsProvider, SessionId, SessionState,
};
use core_event::EventDispatcher;
use core_event::event::{
    ChannelEvent, CoreEvent, SessionEvent, TransferDirection, TransferEvent,
};
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
pub(crate) fn parse_hash_output(cmd: &HashCommand, out: &str) -> Option<String> {
    match cmd {
        HashCommand::Sha256sum | HashCommand::Shasum => {
            out.split_whitespace().next().map(|s| s.to_string())
        }
        HashCommand::Openssl | HashCommand::BsdSha256 => out
            .split('=')
            .nth(1)
            .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
            .filter(|s| !s.is_empty()),
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
    CloseAllChannels,
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
    hash_cmd: Option<HashCommand>,     // 远端哈希命令探测结果缓存（会话级）
    // 传输中到达的断开/关闭（延迟处理）：cancel 置位立即中止传输，但连接释放必须等
    // 传输栈弹出（其局部 ssh2::File drop 会解引用 session，提前释放即 use-after-free）
    pending_disconnect: Option<String>,
    pending_close: bool,
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
            hash_cmd: None,
            pending_disconnect: None,
            pending_close: false,
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
            self.pending_disconnect = Some(reason.to_string());
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
        let entries = sftp_retry_io(&mut self.raw_channels, &self.cancel, "readdir", |sftp| {
            sftp.readdir(std::path::Path::new(path))
        })?;
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
        sftp_retry_io(&mut self.raw_channels, &self.cancel, "mkdir", |sftp| {
            sftp.mkdir(std::path::Path::new(path), 0o755)
        })?;
        Ok(())
    }

    /// 删除远程文件或目录（unlink / rmdir）
    fn sftp_remove_inner(&mut self, path: &str, is_dir: bool) -> CoreResult<()> {
        if is_dir {
            sftp_retry_io(&mut self.raw_channels, &self.cancel, "rmdir", |sftp| {
                sftp.rmdir(std::path::Path::new(path))
            })?;
        } else {
            sftp_retry_io(&mut self.raw_channels, &self.cancel, "unlink", |sftp| {
                sftp.unlink(std::path::Path::new(path))
            })?;
        }
        Ok(())
    }

    /// 重命名远程文件或目录
    fn sftp_rename_inner(&mut self, old: &str, new: &str) -> CoreResult<()> {
        let old_path = std::path::Path::new(old);
        let new_path = std::path::Path::new(new);
        sftp_retry_io(&mut self.raw_channels, &self.cancel, "rename", |sftp| {
            sftp.rename(old_path, new_path, None)
        })?;
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
        let total = sftp_retry_io(&mut self.raw_channels, &self.cancel, "stat", |sftp| {
            sftp.stat(std::path::Path::new(remote_path))
        })?
        .size
        .unwrap_or(0);
        // 创建本地文件
        let mut local_file = std::fs::File::create(local_path)
            .map_err(|e| CoreError::Internal(format!("create local file failed: {e}")))?;
        // 打开远程文件（File 持有会话引用，不借用 sftp 句柄）
        let mut file = sftp_retry_io(&mut self.raw_channels, &self.cancel, "open", |sftp| {
            sftp.open(std::path::Path::new(remote_path))
        })?;
        let mut done: u64 = 0;
        let mut chunk = [0u8; SFTP_CHUNK_SIZE];
        loop {
            // 嵌套处理已到达命令（断开/输入/列目录等立即响应）
            self.drain_nested_commands();
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                // 取消（断开/Close）：显式错误走统一失败清理，避免 total==0 时误报成功
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            // 读块（EAGAIN 退避统一走 io_retry：libssh2 通道顺序约束，退避期间不得处理其他通道）
            let n = io_retry(|| file.read(&mut chunk), &self.cancel)
                .map_err(|e| CoreError::Internal(format!("sftp read failed: {e}")))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&chunk[..n])
                .map_err(|e| CoreError::Internal(format!("local write failed: {e}")))?;
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

    // 注意（libssh2 通道顺序约束）：非阻塞模式下，一个通道的操作未完成（EAGAIN）时
    // 不得操作其他通道（"operations on one channel should complete before operations on
    // another begin"）——EAGAIN 退避期间若处理命令队列（如终端输入写 shell 通道），
    // 会破坏 libssh2 内部状态机导致连接错误（实测 SOCKET_SEND 失败断连）。
    // 因此传输的读/写统一走 io_retry（哑退避），命令队列只在 chunk 边界（无未完成操作）处理。
    // 传输期间终端输入的网络发送可能因带宽竞争排队（SSH 传输特性），属正常现象。

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
        let mut file = sftp_retry_io(&mut self.raw_channels, &self.cancel, "create", |sftp| {
            sftp.create(std::path::Path::new(remote_path))
        })?;
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
                let n = local_file
                    .read(&mut chunk)
                    .map_err(|e| CoreError::Internal(format!("local read failed: {e}")))?;
                if n == 0 {
                    break;
                }
                // 写入完整块（EAGAIN 与零进度统一由 write_full 处理；退避期间不得处理其他通道）
                write_full(&self.cancel, |buf| file.write(buf), &chunk[..n])
                    .map_err(|e| CoreError::Internal(format!("sftp write failed: {e}")))?;
                done += n as u64;
                let _ = progress.send((done, total));
            }
            // 完整性校验：远端文件大小必须与本地一致
            let remote_size =
                sftp_retry_io(&mut self.raw_channels, &self.cancel, "stat", |sftp| {
                    sftp.stat(std::path::Path::new(remote_path))
                })?
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
                    // 传输进行中：断开延迟（disconnect_inner 内部处理），关闭状态与线程
                    // 退出由 flush_pending 在传输栈弹出后统一完成（保持事件顺序）
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
                // 传输中到达的断开/关闭在传输栈弹出后冲刷（延迟释放连接，防 use-after-free）
                self.flush_pending();
                let _ = reply.send(r);
            }
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
            Ok(0) => Ok(()), // EOF：通道已由远端关闭，正常结束
            Ok(n) => {
                let data = Vec::from(&buf[..n]);
                self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                    session_id: self.session_id(),
                    channel_id: channel,
                    data,
                }));
                Ok(())
            }
            Err(e) => {
                if crate::ssh::is_would_block(&e) {
                    return Ok(()); // EAGAIN：无事
                }
                // 真实读错误：连接已死，主动断开并广播（原因供 UI 展示）
                let msg = format!("channel read failed: {e}");
                tracing::warn!(session_id = ?self.session_id(), channel_id = ?channel, error = %e, "shell read failed, disconnecting");
                self.disconnect_inner(&msg);
                Err(CoreError::Internal(msg))
            }
        }
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
                // 零进度：写窗口满，短退避后重试；断开（cancel 置位）立即中止
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
}
