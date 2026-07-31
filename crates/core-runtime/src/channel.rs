//! SSH 通道管理模块
//! 封装 ssh2::Channel 和 ssh2::Sftp，提供统一的读写和生命周期管理接口
//! 所有阻塞 I/O 操作在 spawn_blocking 中执行

use core_common::{ChannelId, ChannelState, ChannelType, CoreResult, SessionId};
use core_event::event::{ChannelEvent, CoreEvent};
use core_event::EventDispatcher;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 内部通道类型，统一包装 ssh2::Channel 和 ssh2::Sftp
pub enum ChannelInner {
    Session(ssh2::Channel),
    Sftp(ssh2::Sftp),
}

/// SSH 数据通道，封装单个 ssh2 Channel 或 Sftp 实例
/// 使用 Arc<Mutex<Option<>>> 确保并发读写安全：spawn_blocking 中持有锁
pub struct Channel {
    pub id: ChannelId,
    pub channel_type: ChannelType,
    pub session_id: SessionId,
    state: RwLock<ChannelState>,
    inner: Arc<std::sync::Mutex<Option<ChannelInner>>>,
    dispatcher: Arc<dyn EventDispatcher>,
}

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

        inner_ch
            .shell()
            .map_err(|e| core_common::CoreError::Internal(format!("start shell failed: {e}")))?;

        {
            let mut guard = channel.inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            *guard = Some(ChannelInner::Session(inner_ch));
        }

        {
            let mut state = channel.state.blocking_write();
            *state = ChannelState::Open;
        }

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: channel.session_id,
            channel_id: channel.id,
        }));

        tracing::info!(channel_id = %channel.id, session_id = %session_id, "shell channel opened");
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

        {
            let mut guard = channel.inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            *guard = Some(ChannelInner::Sftp(sftp));
        }

        {
            let mut state = channel.state.blocking_write();
            *state = ChannelState::Open;
        }

        channel.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: channel.session_id,
            channel_id: channel.id,
        }));

        tracing::info!(channel_id = %channel.id, session_id = %session_id, "sftp channel opened");
        Ok(channel)
    }

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
            let mut ch_inner = guard.take().ok_or_else(|| {
                core_common::CoreError::Internal("channel not open".into())
            })?;

            let result = match &mut ch_inner {
                ChannelInner::Session(ref mut ch) => {
                    let mut buf = vec![0u8; len];
                    ch.read(&mut buf).map(|n| {
                        buf.truncate(n);
                        buf
                    }).map_err(|e| {
                        core_common::CoreError::Internal(format!("channel read failed: {e}"))
                    })
                }
                ChannelInner::Sftp(_) => Err(core_common::CoreError::Internal(
                    "read not supported on sftp channel".into(),
                )),
            };

            *guard = Some(ch_inner);
            result
        })
        .await
        .map_err(|e| core_common::CoreError::Internal(format!("spawn_blocking failed: {e}")))??;

        dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
            session_id,
            channel_id,
            data: data.clone(),
        }));

        Ok(data)
    }

    pub async fn write(&self, data: Vec<u8>) -> CoreResult<usize> {
        if !self.is_open() {
            return Err(core_common::CoreError::Internal("channel is closed".into()));
        }

        let inner = self.inner.clone();

        tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock channel inner failed: {e}"))
            })?;
            let mut ch_inner = guard.take().ok_or_else(|| {
                core_common::CoreError::Internal("channel not open".into())
            })?;

            let result = match &mut ch_inner {
                ChannelInner::Session(ref mut ch) => {
                    ch.write(&data).map_err(|e| {
                        core_common::CoreError::Internal(format!("channel write failed: {e}"))
                    })
                }
                ChannelInner::Sftp(_) => Err(core_common::CoreError::Internal(
                    "write not supported on sftp channel".into(),
                )),
            };

            *guard = Some(ch_inner);
            result
        })
        .await
        .map_err(|e| core_common::CoreError::Internal(format!("spawn_blocking failed: {e}")))?
    }

    pub fn close(&self) -> CoreResult<()> {
        {
            let state = self.state.blocking_read();
            if *state == ChannelState::Closed {
                return Ok(());
            }
        }

        {
            let mut state = self.state.blocking_write();
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
            let mut state = self.state.blocking_write();
            *state = ChannelState::Closed;
        }

        self.dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Closed {
            session_id: self.session_id,
            channel_id: self.id,
        }));

        tracing::info!(channel_id = %self.id, "channel closed");
        Ok(())
    }

    pub fn state(&self) -> ChannelState {
        *self.state.blocking_read()
    }

    pub fn is_open(&self) -> bool {
        *self.state.blocking_read() == ChannelState::Open
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
