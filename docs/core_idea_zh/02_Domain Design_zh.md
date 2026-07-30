# 领域设计

> 定义项目的核心业务领域。

本文档描述**系统是什么**，而非**系统如何实现**。

实现细节、crate 布局、数据库模式和 IPC 接口将在后续文档中定义。

---

# 设计理念

本项目遵循面向领域的设计。

每个模块都应代表 SSH 工作流中的一个真实概念。

一个领域应当：

- 拥有明确的职责。
- 持有自己的状态。
- 暴露最小化的公共接口。
- 独立于 UI。
- 独立于存储实现。

任何领域都不应仅因实现便利而存在。

---

# 领域总览

本项目由以下核心领域组成。

```
Application

├── Session
├── Connection
├── Channel
├── Terminal
├── SFTP
├── Transfer
├── Host
├── HostKey
├── Credential
├── Configuration
├── History
├── Event
└── Storage
```

每个领域持有系统中一个特定部分。

---

# Application 领域

Application 领域代表整个运行时。

职责：

- 应用程序生命周期
- 全局状态
- 启动
- 关闭
- 模块初始化
- 全局事件分发

Application 领域不包含 SSH 逻辑。

它仅协调其他领域。

---

# Session 领域

一个 Session 代表一个用户连接条目。

一个 Session 包含：

- 主机信息
- 认证方式
- 用户偏好
- 窗口状态
- 运行时状态

一个 Session **不**持有 SSH 连接。

Session 是一个逻辑对象。

示例：

```
Session

↓

Connect

↓

Create Connection
```

销毁一个 Connection 不会删除 Session。

---

# Connection 领域

一个 Connection 代表一个活跃的 SSH 传输层。

职责：

- TCP 连接
- SSH 握手
- 认证
- KeepAlive（保活）
- 断开连接
- 重新连接（未来）

Connection 仅在连接期间存在。

一个 Session 在其生命周期中可以创建多个 Connection。

---

# Channel 领域

一个 Channel 代表一个 SSH 通道。

示例：

- 交互式 Shell
- SFTP
- 端口转发（未来）

一个 Channel 始终属于一个 Connection。

一个 Connection 可以持有多个 Channel。

Channel 的生命周期依赖于其所属的 Connection。

---

# Terminal 领域

Terminal 领域代表终端状态。

职责：

- PTY 尺寸
- 终端模式
- 输入流
- 输出流
- 编码
- 剪贴板交互

Terminal 领域不渲染终端内容。

渲染属于 UI。

---

# SFTP 领域

代表远程文件系统。

职责：

- 目录浏览
- 文件元数据
- 上传
- 下载
- 重命名
- 删除
- 创建目录

SFTP 领域从不操作本地 UI 状态。

---

# Transfer 领域

代表长时间运行的文件传输任务。

职责：

- 上传进度
- 下载进度
- 速度
- 预计剩余时间 (ETA)
- 取消
- 重试（未来）

Transfer 独立于 SFTP 浏览。

浏览目录时应保持响应，即使传输正在进行。

---

# Host 领域

代表一台已保存的远程机器。

一个 Host 包含：

- 名称
- 地址
- 端口
- 分组
- 标签（未来）
- 收藏
- 备注（未来）

Host 是持久化的。

即使断开连接，它依然存在。

---

# HostKey 领域

代表受信任的服务器身份。

职责：

- 指纹验证
- 未知主机
- HostKey 已变更
- 信任决策
- OpenSSH 兼容

HostKey 验证必须在认证之前完成。

---

# Credential 领域

代表认证凭据。

支持的认证方式：

- 密码
- 私钥
- SSH Agent（未来）

凭据绝不直接存储在 SQLite 中。

Credential 领域仅与操作系统的安全存储通信。

---

# Configuration 领域

代表应用程序配置。

示例：

- UI 偏好
- SSH 默认值
- SFTP 默认值
- 传输偏好

配置应尽可能保持平台无关。

---

# History 领域

代表本地历史记录。

示例：

- 最近会话
- 连接历史
- 命令历史（未来）
- 传输历史（未来）

History 是本地数据。

History 绝对不得影响运行时行为。

---

# Event 领域

代表 Core 内部的通信。

每个重要的状态转换都成为一个 CoreEvent。

示例：

- Connecting
- Connected
- UploadStarted
- UploadProgress
- HostKeyChanged

Event 领域不持有任何业务逻辑。

它仅传递状态变化。

---

# Storage 领域

代表持久化存储。

职责：

- SQLite
- 数据持久化
- 对象加载
- 对象保存

Storage 对 SSH 一无所知。

Storage 绝不实现业务规则。

---

# 领域关系

```
Application
    │
    ├──────────────┐
    │              │
    ▼              ▼
 Session      Configuration
    │
    ▼
 Connection
    │
    ├──────────────┐
    │              │
    ▼              ▼
 Channel        HostKey
    │
    ├──────────────┐
    │              │
    ▼              ▼
Terminal       SFTP
                   │
                   ▼
              Transfer
```

Credential、Storage 和 Event 是共享的基础设施领域，被多个业务领域使用。

---

# 领域独立性

每个领域应当可独立测试。

示例：

Session 测试绝不应依赖：

- Tauri
- React
- SQLite
- Network（网络）

Connection 测试绝不应依赖：

- UI
- IPC

Storage 测试绝不应依赖：

- SSH

独立的领域更易于维护和复用。

---

# 所有权规则

每个运行时对象有且仅有一个所有者。

| 对象 | 所有者 |
|---------|-------|
| Session | Session 领域 |
| Connection | Connection 领域 |
| SSH Channel | Channel 领域 |
| Terminal State | Terminal 领域 |
| Remote Files | SFTP 领域 |
| File Transfer | Transfer 领域 |
| Saved Host | Host 领域 |
| HostKey | HostKey 领域 |
| Credential | Credential 领域 |
| Configuration | Configuration 领域 |
| History | History 领域 |
| Events | Event 领域 |
| Persistent Data | Storage 领域 |

所有权不得重叠。

---

# 设计目标

此领域模型旨在达成：

- 高内聚
- 低耦合
- 清晰的所有权
- 易于测试
- 长期可维护性
- 可复用的 Core
- 平台独立性

实现细节应始终遵循领域模型，而非反过来。
