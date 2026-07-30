# Core Event

> 定义 Core 的事件模型。

CoreEvent 是 Core 内部用于通知状态变化的唯一机制。

Core 从不直接更新 UI。

UI 观察 CoreEvent 并决定如何响应。

---

# 设计目标

事件系统旨在达成：

- 松耦合
- 高内聚
- 异步通信
- 可预测的状态转换
- 平台独立性
- 可测试性

事件代表事实。

事件绝不代表命令。

---

# 核心原则

## 原则 1

事件描述**已经发生的事情**。

而非应当发生的事情。

正确：

```
ConnectionEstablished
```

错误：

```
ConnectNow
```

---

## 原则 2

事件是不可变的。

一旦发出，事件绝不得被修改。

---

## 原则 3

事件绝不包含业务逻辑。

它们仅携带状态。

---

## 原则 4

事件归属于领域。

每个领域持有自己的事件。

不应存在全局事件列表。

---

# 事件层级

```
CoreEvent

├── ApplicationEvent
├── SessionEvent
├── ConnectionEvent
├── ChannelEvent
├── TerminalEvent
├── TransferEvent
├── HostEvent
├── HostKeyEvent
├── CredentialEvent
├── ConfigurationEvent
├── HistoryEvent
└── SystemEvent
```

---

# ApplicationEvent

代表应用程序生命周期。

示例：

- Started
- Ready
- ShutdownRequested
- ShutdownCompleted

---

# SessionEvent

代表逻辑会话生命周期。

示例：

- Created
- Initialized
- Activated
- Closed

---

# ConnectionEvent

代表 SSH 传输层生命周期。

示例：

```
Connecting

TCPConnected

HandshakeStarted

HostKeyVerifying

Authenticated

Ready

Disconnected

ReconnectStarted

ReconnectSucceeded

ReconnectFailed
```

---

# ChannelEvent

代表 SSH 通道生命周期。

示例：

- Opened
- Ready
- Closed

---

# TerminalEvent

代表终端运行时状态。

示例：

- PTYResized
- OutputReceived
- ClipboardRequested
- TitleChanged

---

# TransferEvent

代表文件传输状态。

示例：

- Started
- Progress
- Paused
- Cancelled
- Finished
- Failed

TransferProgress 应包含：

- SessionID
- TransferID
- BytesTransferred
- TotalBytes
- Speed
- ETA

---

# HostEvent

代表已保存主机的变更。

示例：

- Created
- Updated
- Deleted
- Imported
- Exported

---

# HostKeyEvent

代表服务器身份验证。

示例：

- Unknown
- Changed
- Accepted
- Rejected

HostKeyChanged 应包含：

- HostID
- OldFingerprint
- NewFingerprint

---

# CredentialEvent

代表凭据操作。

示例：

- Loaded
- Updated
- Deleted
- AccessDenied

凭据值绝不得出现在事件中。

---

# ConfigurationEvent

代表配置变更。

示例：

- Loaded
- Updated
- Saved

---

# HistoryEvent

代表历史记录更新。

示例：

- CommandRecorded
- SessionRecorded
- TransferRecorded

---

# SystemEvent

代表基础设施变更。

示例：

- DatabaseOpened
- DatabaseError
- FileSystemError
- NetworkUnavailable

---

# 事件流

每个操作都遵循相同的模式。

```
User

↓

IPC Request

↓

Core

↓

Business Logic

↓

State Updated

↓

CoreEvent

↓

IPC Event

↓

UI
```

事件始终在状态变化之后发出。

---

# 事件排序

事件必须保持时间顺序。

示例：

```
Connecting

↓

TCPConnected

↓

HandshakeStarted

↓

HostKeyVerified

↓

Authenticated

↓

Ready
```

消费者绝不应以乱序接收到这些事件。

---

# 事件所有权

每个事件恰好归属于一个领域。

| 事件 | 所有者 |
|---------|-------|
| SessionEvent | Session 领域 |
| ConnectionEvent | Connection 领域 |
| ChannelEvent | Channel 领域 |
| TransferEvent | Transfer 领域 |
| HostEvent | Host 领域 |
| HostKeyEvent | HostKey 领域 |
| CredentialEvent | Credential 领域 |
| ConfigurationEvent | Configuration 领域 |
| HistoryEvent | History 领域 |
| ApplicationEvent | Application 领域 |
| SystemEvent | 基础设施 |

所有权不得重叠。

---

# 事件载荷

每个事件应仅包含最少量的必要信息。

事件应通过 ID 引用运行时对象。

示例：

```
TransferProgress

SessionID

TransferID

BytesTransferred

TotalBytes
```

大型运行时对象绝不应被复制到事件中。

---

# 事件可靠性

事件应当是：

- 有序的
- 轻量级的
- 不可变的
- 非阻塞的

事件系统绝不能成为瓶颈。

---

# 事件消费者

事件可被以下消费者使用：

- IPC 层
- 日志记录器
- 指标采集
- 未来的插件
- CLI
- 自动化测试

Core 不应知晓谁在消费事件。

---

# 事件规则

事件绝不得：

- 修改状态
- 直接触发 UI
- 执行阻塞操作
- 持有资源

事件仅仅是通知。

---

# 总结

Core 遵循事件驱动架构。

每个重要的状态转换都成为一个领域事件。

Core 持有状态。

事件暴露状态。

消费者决定如何响应。

这种设计使 Core 保持可复用、可测试，并独立于任何特定的前端。
