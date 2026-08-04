//! SSH 通道管理模块
//! 封装 ssh2::Channel 和 ssh2::Sftp，提供统一的读写和生命周期管理接口
//!
//! 关键设计：非阻塞 I/O 模式
//!   shell() 之后调用 session.set_blocking(false)，所有读写立即返回不阻塞。
//!   读循环在无数据时 sleep 10ms 后重试，写操作在 spawn_blocking 中直接执行。
//!   因为 I/O 非阻塞，锁持有时间极短（微秒级），读写无锁竞争。

use core_common::{ChannelId, ChannelState, ChannelType, CoreResult, PtySize, SessionId};
use core_event::event::{ChannelEvent, CoreEvent};
use core_event::EventDispatcher;
use std::io::{Read, Write};
use std::sync::{Arc, RwLock};

/// 内部通道类型，统一包装 ssh2::Channel 和 ssh2::Sftp
enum ChannelInner {
    Session(ssh2::Channel),
    Sftp(ssh2::Sftp),
}

/// SSH 数据通道
/// 使用 Arc<Mutex<Option<>>> 确保并发安全：spawn_blocking 中持有锁
/// 由于 I/O 是非阻塞的，锁持有时间极短
pub struct Channel {
    pub id: ChannelId,
    pub channel_type: ChannelType,
    pub session_id: SessionId,
    state: RwLock<ChannelState>,
    inner: Arc<std::sync::Mutex<Option<ChannelInner>>>,
    dispatcher: Arc<dyn EventDispatcher>,
}

/// PTY 终端模式 opcode 常量（参照 libssh2 定义）
const ECHO: u8 = 53;
const ICANON: u8 = 2;
const ISIG: u8 = 3;
const ICRNL: u8 = 36;
const ONLCR: u8 = 72;
const OPOST: u8 = 70;

impl Channel {
    pub fn open_shell(
        session_id: SessionId,
        ssh_session: &ssh2::Session,
        dispatcher: Arc<dyn EventDispatcher>,
    ) -> CoreResult<Arc<Self>> {
        let channel = Self::create(session_id, ChannelType::Shell, dispatcher.clone())?;

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
            session_id: channel.session_id,
            channel_id: channel.id,
            channel_type: channel.channel_type,
        }));

        let mut inner_ch = ssh_session
            .channel_session()
            .map_err(|e| core_common::CoreError::Internal(format!("open channel session failed: {e}")))?;

        // 设置标准 PTY 终端模式（与 OpenSSH 行为对齐）
        // 必须显式设置模式，否则服务端默认值可能关闭回显等关键功能
        let mut modes = ssh2::PtyModes::new();
        modes.set_boolean(ECHO, true);
        modes.set_boolean(ICANON, true);
        modes.set_boolean(ISIG, true);
        modes.set_boolean(ICRNL, true);
        modes.set_boolean(ONLCR, true);
        modes.set_boolean(OPOST, true);

        let pty = PtySize::default();
        inner_ch
            .request_pty("xterm-256color", Some(modes), Some((pty.cols, pty.rows, pty.width_px, pty.height_px)))
            .map_err(|e| core_common::CoreError::Internal(format!("request pty failed: {e}")))?;

        inner_ch
            .shell()
            .map_err(|e| core_common::CoreError::Internal(format!("start shell failed: {e}")))?;

        // 切换到非阻塞模式：所有读写立即返回，彻底消除锁竞争
        ssh_session.set_blocking(false);

        {
            let mut guard = channel.inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            *guard = Some(ChannelInner::Session(inner_ch));
        }

        {
            let mut state = channel.state.write().unwrap();
            *state = ChannelState::Open;
        }

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: channel.session_id,
            channel_id: channel.id,
        }));

        tracing::info!(channel_id = %channel.id, session_id = %session_id, "shell channel opened (nonblocking)");
        Ok(channel)
    }

    pub fn open_sftp(
        session_id: SessionId,
        ssh_session: &ssh2::Session,
        dispatcher: Arc<dyn EventDispatcher>,
    ) -> CoreResult<Arc<Self>> {
        let channel = Self::create(session_id, ChannelType::Sftp, dispatcher.clone())?;

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
            session_id: channel.session_id,
            channel_id: channel.id,
            channel_type: channel.channel_type,
        }));

        let sftp = ssh_session
            .sftp()
            .map_err(|e| core_common::CoreError::Internal(format!("open sftp failed: {e}")))?;

        ssh_session.set_blocking(false);

        {
            let mut guard = channel.inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            *guard = Some(ChannelInner::Sftp(sftp));
        }

        {
            let mut state = channel.state.write().unwrap();
            *state = ChannelState::Open;
        }

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: channel.session_id,
            channel_id: channel.id,
        }));

        tracing::info!(channel_id = %channel.id, session_id = %session_id, "sftp channel opened");
        Ok(channel)
    }

    /// 读取通道数据（非阻塞模式）
    /// 有数据时返回数据并发送 DataReceived 事件；无数据时返回空 Vec
    pub async fn read(&self, len: usize) -> CoreResult<Vec<u8>> {
        if !self.is_open() {
            return Err(core_common::CoreError::Internal("channel is closed".into()));
        }

        let inner = self.inner.clone();
        let channel_id = self.id;
        let session_id = self.session_id;
        let dispatcher = self.dispatcher.clone();

        let data = tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            let ch_inner = guard.as_mut().ok_or_else(|| {
                core_common::CoreError::Internal("channel not open".into())
            })?;

            match ch_inner {
                ChannelInner::Session(ref mut ch) => {
                    let mut buf = vec![0u8; len];
                    match ch.read(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            Ok(buf)
                        }
                        Err(e) => {
                            // 非阻塞模式下无数据时返回 EAGAIN(-37)，消息含 "would block"
                            let msg = format!("{e}").to_lowercase();
                            if msg.contains("would block") || msg.contains("-37") {
                                Ok(Vec::new())
                            } else {
                                Err(core_common::CoreError::Internal(format!(
                                    "channel read failed: {e}"
                                )))
                            }
                        }
                    }
                }
                ChannelInner::Sftp(_) => Err(core_common::CoreError::Internal(
                    "read not supported on sftp channel".into(),
                )),
            }
        })
        .await
        .map_err(|e| core_common::CoreError::Internal(format!("spawn_blocking failed: {e}")))??;

        if !data.is_empty() {
            dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                session_id,
                channel_id,
                data: data.clone(),
            }));
        }

        Ok(data)
    }

    /// 向通道写入数据（非阻塞模式）
    pub async fn write(&self, data: Vec<u8>) -> CoreResult<usize> {
        if !self.is_open() {
            return Err(core_common::CoreError::Internal("channel is closed".into()));
        }

        let inner = self.inner.clone();

        tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            let ch_inner = guard.as_mut().ok_or_else(|| {
                core_common::CoreError::Internal("channel not open".into())
            })?;

            match ch_inner {
                ChannelInner::Session(ref mut ch) => ch.write(&data).map_err(|e| {
                    core_common::CoreError::Internal(format!("channel write failed: {e}"))
                }),
                ChannelInner::Sftp(_) => Err(core_common::CoreError::Internal(
                    "write not supported on sftp channel".into(),
                )),
            }
        })
        .await
        .map_err(|e| core_common::CoreError::Internal(format!("spawn_blocking failed: {e}")))?
    }

    /// 关闭通道
    /// 因非阻塞模式下锁立即可用，不会卡住
    pub fn close(&self) -> CoreResult<()> {
        {
            let state = self.state.read().unwrap();
            if *state == ChannelState::Closed || *state == ChannelState::Closing {
                return Ok(());
            }
        }

        {
            let mut state = self.state.write().unwrap();
            *state = ChannelState::Closing;
        }

        let mut guard = self.inner.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
        })?;

        if let Some(inner) = guard.take() {
            match inner {
                ChannelInner::Session(mut ch) => {
                    let _ = ch.close();
                    let _ = ch.wait_close();
                }
                ChannelInner::Sftp(_sftp) => {}
            }
        }

        {
            let mut state = self.state.write().unwrap();
            *state = ChannelState::Closed;
        }

        self.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Closed {
            session_id: self.session_id,
            channel_id: self.id,
        }));

        tracing::info!(channel_id = %self.id, "channel closed");
        Ok(())
    }

    /// 调整终端 PTY 尺寸
    /// 用于响应前端窗口尺寸变化
    pub fn resize_pty(&self, cols: u32, rows: u32) -> CoreResult<()> {
        if !self.is_open() {
            return Err(core_common::CoreError::Internal("channel is closed".into()));
        }

        tokio::task::block_in_place(|| {
            let mut guard = self.inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            if let Some(ChannelInner::Session(ref mut ch)) = *guard {
                ch.request_pty_size(cols, rows, Some(cols * 8), Some(rows * 16))
                    .map_err(|e| {
                        core_common::CoreError::Internal(format!("resize pty failed: {e}"))
                    })?;
            }
            Ok(())
        })
    }

    /// 启动后台读取循环
    /// 非阻塞轮询：有数据时立即处理，无数据时 sleep 10ms 后重试
    pub fn start_read_loop(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let channel = self.clone();
        tokio::spawn(async move {
            loop {
                if !channel.is_open() {
                    break;
                }
                match channel.read(4096).await {
                    Ok(data) => {
                        if data.is_empty() {
                            // 非阻塞模式无数据，短暂休眠后重试
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            continue;
                        }
                    }
                    Err(e) => {
                        tracing::debug!(channel_id = %channel.id, error = %e, "read loop exiting");
                        break;
                    }
                }
            }
        })
    }

    pub fn state(&self) -> ChannelState {
        *self.state.read().unwrap()
    }

    pub fn is_open(&self) -> bool {
        *self.state.read().unwrap() == ChannelState::Open
    }

    fn create(
        session_id: SessionId,
        channel_type: ChannelType,
        dispatcher: Arc<dyn EventDispatcher>,
    ) -> CoreResult<Arc<Self>> {
        Ok(Arc::new(Self {
            id: ChannelId::new(),
            channel_type,
            session_id,
            state: RwLock::new(ChannelState::Opening),
            inner: Arc::new(std::sync::Mutex::new(None)),
            dispatcher,
        }))
    }
}
