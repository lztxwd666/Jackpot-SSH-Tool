# Core API & IPC

> Define the communication contract between the frontend and the Core.

The Core exposes a stable API.

IPC is only one implementation of this API.

The Core must never depend on any specific IPC technology.

---

# Design Philosophy

The Core is an independent runtime.

Frontend applications communicate with the Core exclusively through Core APIs.

Desktop applications use IPC.

Other frontends may use different adapters.

The communication model remains identical.

---

# Architecture

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

Future frontends:

```
CLI
HTTP
VSCode
JetBrains
Tests
```

can all communicate with the same Core API.

---

# Design Principles

## Principle 1

Core owns the API.

Adapters implement the API.

The Core must never know:

- Tauri
- invoke()
- JavaScript
- JSON

---

## Principle 2

Every API belongs to one Domain.

Example:

```
Session API

Connection API

Transfer API

Host API

Configuration API
```

No "Misc API" should exist.

---

## Principle 3

API performs actions.

Events report results.

Requests flow downward.

Events flow upward.

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

## Principle 4

API returns only immediate results.

Long-running state changes are reported by CoreEvent.

Example:

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

The API does not wait for the connection to complete.

---

# Communication Model

The system follows an asynchronous request-event model.

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

The frontend never polls.

The frontend reacts to events.

---

# API Categories

The Core exposes multiple logical APIs.

```
Application API

Session API

Connection API

Terminal API

Transfer API

Host API

Configuration API
```

Each API owns one domain.

---

# API Design Rules

Every API should:

- Have a single responsibility.
- Be deterministic.
- Never expose infrastructure.
- Never expose implementation details.
- Never return runtime objects.

---

# API Return Values

APIs return only:

```
Success

Failure

Immediate Result
```

Example:

```
CreateSession

↓

SessionID
```

NOT

```
Connection State

Authentication Progress

Upload Progress
```

Those belong to events.

---

# Event Subscription

The frontend subscribes to CoreEvent once.

```
Application Start

↓

Subscribe CoreEvent

↓

Receive Events Forever
```

The frontend never subscribes to individual modules.

---

# Event Routing

Core Runtime emits events.

IPC forwards events.

Frontend consumes events.

```
Core Runtime

↓

CoreEvent

↓

IPC Adapter

↓

Frontend
```

The adapter never modifies events.

---

# Domain Isolation

The frontend communicates with domains independently.

Example:

```
Host API

↓

Session API

↓

Transfer API
```

One domain should never require another domain's API.

---

# Object Identity

Runtime objects are referenced only by IDs.

Example:

```
SessionID

ConnectionID

TransferID

ChannelID
```

Runtime pointers are never exposed.

---

# Threading

API calls should return immediately whenever possible.

Long-running operations execute asynchronously.

Progress is reported through events.

The frontend should never block waiting for runtime state.

---

# Error Handling

Infrastructure errors remain inside the Core.

The frontend receives only business-level errors.

Example:

Instead of:

```
SQLite Busy
```

Return:

```
HostSaveFailed
```

Instead of:

```
Socket Closed
```

Return:

```
ConnectionLost
```

---

# Version Compatibility

Core API should remain stable.

Adapters may evolve independently.

Future IPC implementations should not require changes to business domains.

---

# Future Adapters

Possible adapters include:

```
Tauri IPC

CLI

HTTP API

VSCode Extension

JetBrains Plugin

Testing Adapter
```

All adapters communicate with the same Core API.

---

# Summary

The communication model consists of two independent flows.

```
Frontend

↓

Core API

↓

Core
```

Action flow.

```
Core

↓

CoreEvent

↓

Frontend
```

State flow.

The API performs actions.

The Event system communicates state.

Keeping these two responsibilities separate ensures a clean, scalable and reusable architecture.
