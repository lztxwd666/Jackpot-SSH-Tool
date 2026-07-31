# Core API 与 IPC

> 定义前端与 Core 之间的通信契约。

Core 暴露一个稳定的 API。

IPC 只是此 API 的一种实现方式。

Core 绝不得依赖任何特定的 IPC 技术。

---

# 设计理念

Core 是一个独立的运行时。

前端应用程序仅通过 Core API 与 Core 通信。

桌面应用程序使用 IPC。

其他前端可使用不同的适配器。

通信模型保持一致。

---

# 架构

```
React UI
      │
      ▼
 IPC Client
      │
      ▼
Tauri Adapter
      │
      ▼
 Core API
      │
      ▼
Core Runtime
```

未来的前端：

```
CLI
HTTP
VSCode
JetBrains
Tests
```

均可与同一个 Core API 通信。

---

# 设计原则

## 原则 1

Core 持有 API。

适配器实现 API。

Core 绝不得知晓：

- Tauri
- invoke()
- JavaScript
- JSON

---

## 原则 2

每个 API 归属于一个领域。

示例：

```
Session API

Connection API

Transfer API

Host API

Configuration API
```

不应存在"杂项 API"。

---

## 原则 3

API 执行操作。

事件报告结果。

请求向下流动。

事件向上流动。

```
Request

↓

Core

↓

State Change

↓

CoreEvent

↓

Frontend
```

---

## 原则 4

API 仅返回即时结果。

长时间运行的状态变化由 CoreEvent 报告。

示例：

```
connect()

↓

Accepted

↓

Connecting...

↓

Connected

↓

ShellReady
```

API 不等待连接完成。

---

# 通信模型

系统遵循异步的请求-事件模型。

```
Frontend

↓

Request

↓

Core API

↓

Business Logic

↓

Runtime State

↓

CoreEvent

↓

Frontend
```

前端从不轮询。

前端对事件做出响应。

---

# API 分类

Core 暴露多个逻辑 API。

```
Application API

Session API

Connection API

Terminal API

Transfer API

Host API

Configuration API
```

每个 API 持有一个领域。

---

# API 设计规则

每个 API 应当：

- 具有单一职责。
- 是确定性的。
- 绝不暴露基础设施。
- 绝不暴露实现细节。
- 绝不返回运行时对象。

---

# API 返回值

API 仅返回：

```
Success

Failure

Immediate Result
```

示例：

```
CreateSession

↓

SessionID
```

而非：

```
Connection State

Authentication Progress

Upload Progress
```

这些属于事件。

---

# 事件订阅

前端一次性订阅 CoreEvent。

```
Application Start

↓

Subscribe CoreEvent

↓

Receive Events Forever
```

前端绝不对单个模块进行订阅。

---

# 事件路由

Core Runtime 发出事件。

IPC 转发事件。

前端消费事件。

```
Core Runtime

↓

CoreEvent

↓

IPC Adapter

↓

Frontend
```

适配器绝不修改事件。

---

# 领域隔离

前端与各个领域独立通信。

示例：

```
Host API

↓

Session API

↓

Transfer API
```

一个领域绝不应依赖另一个领域的 API。

---

# 对象标识

运行时对象仅通过 ID 引用。

示例：

```
SessionID

ConnectionID

TransferID

ChannelID
```

运行时指针绝不暴露。

---

# 线程

API 调用应尽可能立即返回。

长时间运行的操作异步执行。

进度通过事件报告。

前端绝不应阻塞等待运行时状态。

---

# 错误处理

基础设施错误保留在 Core 内部。

前端仅接收业务级别的错误。

示例：

不应返回：

```
SQLite Busy
```

而应返回：

```
HostSaveFailed
```

不应返回：

```
Socket Closed
```

而应返回：

```
ConnectionLost
```

---

# 版本兼容性

Core API 应保持稳定。

适配器可独立演进。

未来的 IPC 实现不应要求更改业务领域。

---

# 未来的适配器

可能的适配器包括：

```
Tauri IPC

CLI

HTTP API

VSCode Extension

JetBrains Plugin

Testing Adapter
```

所有适配器均与同一个 Core API 通信。

---

# 总结

通信模型由两个独立的流向组成。

```
Frontend

↓

Core API

↓

Core
```

操作流。

```
Core

↓

CoreEvent

↓

Frontend
```

状态流。

API 执行操作。

事件系统传递状态。

将这两项职责分离，确保了架构的清晰、可扩展和可复用。
