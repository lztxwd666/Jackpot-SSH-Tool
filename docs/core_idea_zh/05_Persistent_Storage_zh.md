# 持久化存储

> 定义用户数据如何持久化，同时保持与现有 SSH 生态系统的完全兼容。

持久化存储是一项基础设施服务。

其唯一职责是保存用户拥有的数据。

业务逻辑绝不依赖存储实现。

---

# 设计理念

持久化存储存在的目的是保存用户数据。

它**不**负责：

- 业务逻辑
- 运行时状态
- 会话生命周期
- SSH 协议
- 事件分发

存储是被动的。

Core 持有数据。

存储仅负责持久化数据。

---

# 开放标准优先

当已有既定标准存在时，应采纳而非创建专有格式。

本项目应自然地融入现有的 OpenSSH 生态系统。

用户数据应保持可读、可移植且可在此应用程序之外复用。

---

# 存储架构

```
业务领域

↓

Repository / Provider

↓

存储层

↓

SQLite
OpenSSH 文件
操作系统服务
```

业务领域绝不直接访问存储。

每个持久化操作都经过一个专用的抽象层。

---

# 持久化数据分类

持久化数据分为三类。

```
SQLite
OpenSSH 文件
操作系统服务
```

每个类别承担不同的职责。

---

# SQLite

SQLite 存储应用程序特定数据。

示例包括：

- 已保存的主机
- 分组
- 收藏
- 应用程序配置
- 最近连接
- 传输历史
- 本地命令历史（未来）
- UI 偏好

SQLite 绝不应存储安全敏感的凭据。

---

# OpenSSH 文件

应尽可能复用现有的 OpenSSH 文件，而非引入新格式。

示例：

```
~/.ssh/config

~/.ssh/known_hosts

~/.ssh/id_rsa

~/.ssh/id_ed25519

~/.ssh/authorized_keys
```

在 Windows 上，应使用等效的 OpenSSH 目录。

应用程序应保持与 OpenSSH 所创建文件的兼容。

同样，由本应用程序修改的文件应仍可被 OpenSSH 使用。

---

# 操作系统服务

敏感信息属于操作系统。

示例：

密码

私钥口令

令牌（未来）

平台示例：

Windows

凭据管理器

Linux

Secret Service

macOS

钥匙串

敏感信息绝不得存储在 SQLite 中。

---

# Repository / Provider 职责

持久化操作通过专用的抽象层暴露。

示例：

```
HostRepository

ConfigurationRepository

HistoryRepository

TransferHistoryRepository

SSHConfigProvider

KnownHostsProvider

CredentialProvider
```

Repository 持有应用程序数据。

Provider 集成外部系统。

---

# Repository 与 Provider 的区别

Repository 持有由应用程序管理的数据。

示例：

```
HostRepository

HistoryRepository

ConfigurationRepository
```

Provider 暴露由操作系统或现有标准所拥有的外部资源。

示例：

```
CredentialProvider

KnownHostsProvider

SSHConfigProvider
```

这种区分使业务逻辑独立于基础设施。

---

# 对象所有权

每个持久化对象有且仅有一个所有者。

| 对象 | 所有者 |
|----------|-------|
| 已保存的主机 | HostRepository |
| 分组 | HostRepository |
| 收藏 | HostRepository |
| 配置 | ConfigurationRepository |
| 历史记录 | HistoryRepository |
| 传输历史 | TransferHistoryRepository |
| SSH Config | SSHConfigProvider |
| Known Hosts | KnownHostsProvider |
| 凭据 | CredentialProvider |

所有权不得重叠。

---

# 运行时对象

运行时对象绝不持久化。

示例：

```
Session

Connection

Channel

Transfer Task

运行时缓存

Events

网络状态
```

运行时对象在应用程序退出时消失。

---

# 事务

当多个持久化对象被修改时，Repository 应执行原子更新。

示例：

```
创建 Host

↓

保存 Host

↓

保存分组映射

↓

提交
```

绝不应出现部分持久化的情况。

---

# 标识

持久化对象持有稳定的标识符。

示例：

```
HostID

GroupID

HistoryID

TransferHistoryID
```

标识符永不改变。

关系应始终引用标识符，而非复制对象。

---

# 缓存

Repository 可缓存频繁访问的数据。

缓存是实现细节。

业务逻辑绝不得依赖缓存的存在。

---

# 迁移

SQLite 应支持自动模式迁移。

迁移要求：

- 模式版本
- 自动升级
- 尽可能保持向后兼容

OpenSSH 文件绝不应需要迁移。

它们遵循 OpenSSH 规范。

---

# 备份策略

应用程序应保持易于备份。

推荐的目录布局：

```
Application/

├── config.db
├── logs/
├── cache/
└── temp/
```

SSH 相关文件保留在标准 OpenSSH 目录中。

恢复这两个目录即可恢复完整的应用程序。

---

# 性能原则

持久化操作应：

- 避免不必要的磁盘写入
- 在适当的时候批量更新
- 最小化 I/O
- 尽可能异步执行

存储绝不应阻塞 UI。

---

# 错误处理

基础设施特定的错误绝不应泄漏到业务领域中。

示例：

不应暴露：

```
SQLite Error
```

而应暴露：

```
SaveHostFailed
```

不应暴露：

```
Permission Denied

known_hosts
```

而应暴露：

```
HostKeyUpdateFailed
```

基础设施细节保留在 Repository 和 Provider 内部。

---

# 未来的兼容性

存储架构应支持未来的扩展，而无需更改业务领域。

可能的未来实现：

- SQLite
- 内存存储
- JSON 存储（测试用）
- PostgreSQL（未来）

Core 应独立于存储后端。

---

# 总结

持久化存储由三个独立层组成。

```
应用程序数据
        │
        ▼
SQLite
```

```
SSH 生态系统
        │
        ▼
OpenSSH 文件
```

```
敏感信息
        │
        ▼
操作系统服务
```

应用程序特定信息属于 SQLite。

OpenSSH 兼容信息属于 OpenSSH。

敏感信息属于操作系统。

本项目应尽可能与现有标准集成，而非取代它们。
