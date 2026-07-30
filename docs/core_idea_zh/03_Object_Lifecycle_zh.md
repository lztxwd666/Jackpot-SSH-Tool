# 对象生命周期

> 定义每个运行时对象的生命周期与所有权。

本文档规定对象如何被创建、使用和销毁。

同时也定义了对象之间的关系。

本文档被视为 Core 的运行时规范。

---

# 设计原则

每个对象的生命周期必须满足：

- 单一所有权
- 清晰的职责
- 显式的状态转换
- 可预测的销毁
- 无隐藏副作用

每个对象应具有明确定义的生命周期。

职责不同的对象绝不得共享相同的生命周期。

---

# 运行时对象层级

```
Host
│
├── Session
│     │
│     ├── Connection
│     │      │
│     │      ├── Shell Channel
│     │      ├── SFTP Channel
│     │      └── Future Channels...
│     │
│     └── Runtime State
│
└── Persistent Configuration
```

每一层持有其下一层。

销毁父对象即销毁所有子对象。

销毁子对象绝不会销毁其父对象。

---

# Host 生命周期

Host 代表一台已保存的远程设备。

Host 是持久化的。

它独立于运行时存在。

Host 生命周期：

```
Create

↓

Save

↓

Edit

↓

Reuse

↓

Delete
```

删除一个 Host 会永久移除：

- 连接配置
- 用户偏好
- 分组信息
- 关联的元数据

删除 Host **不**影响活跃的运行时对象。

已有的 Session 在关闭前始终有效。

---

# Session 生命周期

一个 Session 代表一个运行中的工作区。

创建 Session **不**会建立 SSH 连接。

生命周期：

```
Created

↓

Initialized

↓

Waiting

↓

Connecting

↓

Connected

↓

Disconnected

↓

Closed
```

特征：

- 当用户打开一个 Host 时创建。
- 无论网络状态如何始终存在。
- 持有运行时状态。
- 持有业务运行时状态。UI 运行时状态完全属于 UI 层。
- 可多次重新连接。

一个 Session 在其生命周期中可以创建多个 Connection。

一个 Session 恰好持有一个活跃的 Connection。

---

# Connection 生命周期

一个 Connection 代表一个 SSH 传输层。

生命周期：

```
Created

↓

TCP Connected

↓

SSH Handshake

↓

HostKey Verified

↓

Authenticated

↓

Ready

↓

Disconnected

↓

Destroyed
```

Connection 特征：

- 仅在连接期间存在。
- 持有 SSH 传输层。
- 持有认证状态。
- 持有 SSH 通道。
- 绝不持有 Session 数据。

销毁一个 Connection 会销毁其所有 Channel。

---

# Channel 生命周期

一个 Channel 代表一个 SSH 通信通道。

示例：

- 交互式 Shell
- SFTP
- 端口转发
- 未来的扩展

生命周期：

```
Open

↓

Ready

↓

Running

↓

Closing

↓

Closed
```

Channel 始终恰好属于一个 Connection。

Channel 不能独立存在。

---

# 对象所有权

```
Host
 │
 └──── 持有 ───► Session
                     │
                     └──── 持有 ───► Connection
                                            │
                                            └──── 持有 ───► Channel
```

所有权是独占的。

对象不能有多个所有者。

---

# 运行时状态所有权

每个对象仅持有自己的状态。

## Host 持有

- 名称
- 地址
- 端口
- 认证偏好
- 用户设置
- 元数据

---

## Session 持有

- 当前状态
- 活跃终端
- 当前工作目录（可选）
- UI 运行时状态
- 运行时缓存

---

## Connection 持有

- TCP socket
- SSH 传输层
- 加密套件状态
- 认证状态
- KeepAlive
- SSH 通道

---

## Channel 持有

- 通道 ID
- 通道类型
- 流
- PTY（Shell）
- SFTP 句柄（SFTP）

---

# 生命周期规则

## 规则 1

Host 比所有运行时对象存活更久。

```
Host

创建一次

↓

多次复用

↓

手动删除
```

---

## 规则 2

Session 比 Connection 存活更久。

```
Session

↓

Connection A

↓

断开连接

↓

Connection B

↓

断开连接
```

重新连接会创建一个新的 Connection。

Session 保持不变。

---

## 规则 3

Connection 比 Channel 存活更久。

```
Connection

↓

Shell

↓

SFTP

↓

Port Forward
```

关闭 Connection 会关闭所有 Channel。

---

## 规则 4

Channel 从不重新连接。

重新连接会创建新的 Channel。

已销毁的 Channel 绝不复用。

---

# 故障恢复

故障应始终发生在尽可能低的层级。

示例：

```
网络断开

↓

Connection 已销毁

↓

Session 存活

↓

重新连接

↓

新的 Connection

↓

新的 Channel
```

Session 绝不应因临时网络问题而被销毁。

---

# 持久化规则

仅 Host 是持久化的。

其余一切皆为运行时对象。

| 对象 | 持久化 |
|----------|------------|
| Host | 是 |
| Session | 否 |
| Connection | 否 |
| Channel | 否 |

运行时对象绝不得直接写入数据库。

仅用户数据是持久化的。

---

# 运行时标识

每个运行时对象持有一个唯一标识符。

示例：

```
HostID

↓

SessionID

↓

ConnectionID

↓

ChannelID
```

每个 Event 均引用这些标识符。

运行时 ID 绝不复用。

---

# 销毁规则

销毁一个对象始终会销毁其子对象。

```
销毁 Host

↓

销毁 Session

↓

销毁 Connection

↓

销毁 Channel
```

反向销毁是被禁止的。

```
销毁 Channel

×

销毁 Connection
```

不允许。

---

# 未来的兼容性

此生命周期无需修改即可支持未来的功能。

示例：

- SSH Agent
- 端口转发
- 多个 Shell
- 后台传输
- 会话恢复
- 插件

新功能应扩展现有生命周期，而非创建并列的生命周期。

---

# 总结

运行时模型遵循严格的层级结构。

```
Host
    │
Session
    │
Connection
    │
Channel
```

每一层都有：

- 独立的生命周期
- 独立的职责
- 独立的状态
- 单一所有权

此层级结构构成了整个 Core 架构的基石。
