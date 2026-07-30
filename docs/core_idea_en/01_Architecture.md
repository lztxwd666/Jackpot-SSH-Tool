# System Architecture

> Define the overall architecture of the project.

---

# Overview

The project follows a layered architecture.

Business logic, platform integration, and user interface are completely separated.

```
                  User
                    │
                    ▼
        ┌──────────────────────┐
        │      React UI        │
        │                      │
        │  Rendering           │
        │  User Interaction    │
        └──────────┬───────────┘
                   │
                   │ IPC
                   ▼
        ┌──────────────────────┐
        │    Tauri Adapter     │
        │                      │
        │ IPC Translation      │
        │ Window Lifecycle     │
        │ Native Integration   │
        └──────────┬───────────┘
                   │
                   ▼
        ┌──────────────────────┐
        │      SSH Core        │
        │                      │
        │ Session              │
        │ SSH                  │
        │ SFTP                 │
        │ Database             │
        │ Credential           │
        │ HostKey              │
        │ Configuration        │
        │ Event Dispatcher     │
        └──────────┬───────────┘
                   │
                   ▼
        ┌──────────────────────┐
        │   System Services    │
        │                      │
        │ ssh2-rs              │
        │ SQLite               │
        │ OS Credential Store  │
        │ File System          │
        └──────────────────────┘
```

The UI never communicates directly with the operating system.

All operations must go through SSH Core.

---

# Architecture Principles

The project follows four architectural principles.

---

## Principle 1

Core does not know UI.

The Core must never depend on:

- React
- Tauri
- xterm.js
- HTML
- CSS

The Core must be compilable as a standalone Rust library.

---

## Principle 2

UI does not know business logic.

The UI should never:

- Perform SSH operations
- Access SQLite directly
- Read credentials
- Parse SSH configuration
- Manage HostKey
- Implement connection logic

UI only displays state.

---

## Principle 3

IPC is the only communication channel.

Every interaction between UI and Core happens through IPC.

Example:

```
User Click

↓

IPC Request

↓

Core

↓

CoreEvent

↓

IPC Event

↓

UI Refresh
```

Neither side should bypass IPC.

---

## Principle 4

Core communicates through events.

The Core owns every runtime state.

Whenever the state changes:

Core emits a CoreEvent.

Consumers decide how to react.

The Core never updates the interface directly.

---

# Layer Responsibilities

---

## UI Layer

Responsible for:

- Rendering
- Theme
- Window Layout
- Keyboard Shortcuts
- Drag & Drop
- Dialogs
- Notifications
- User Interaction

The UI contains no business logic.

---

## Adapter Layer

Acts as the bridge between UI and Core.

Responsibilities:

- IPC
- Command Routing
- Event Forwarding
- Window Lifecycle
- Platform-specific APIs

No business logic should exist here.

---

## Core Layer

The Core is the heart of the project.

Responsibilities include:

- SSH
- SFTP
- Session
- Configuration
- Credential
- Database
- HostKey
- Runtime State
- Event Dispatching

The Core should be usable without any graphical interface.

---

## Infrastructure Layer

Provides external capabilities.

Examples include:

- SSH implementation
- SQLite
- Credential Manager
- File System
- Network
- Logging

Infrastructure never contains business rules.

---

# Runtime Flow

The application follows a unidirectional flow.

```
User

↓

UI

↓

IPC

↓

Core

↓

Infrastructure

↓

Core

↓

CoreEvent

↓

IPC

↓

UI
```

Business logic always executes inside Core.

---

# Data Ownership

Each type of data has exactly one owner.

| Data | Owner |
|-------|-------|
| Session | Core |
| SSH Connection | Core |
| Runtime State | Core |
| Transfer State | Core |
| HostKey | Core |
| Credential | Core |
| SQLite | Core |
| Window State | UI |
| Theme | UI |
| Layout | UI |

Ownership must never overlap.

---

# Dependency Rules

Dependencies always point downward.

```
UI

↓

Adapter

↓

Core

↓

Infrastructure
```

Reverse dependencies are forbidden.

Examples:

UI → Core

Allowed.

Core → UI

Forbidden.

Infrastructure → Core

Forbidden.

---

# Event Flow

The system is event-driven.

Every important state transition should generate a CoreEvent.

Typical lifecycle:

```
Connect

↓

Connecting

↓

Authenticating

↓

Connected

↓

Shell Ready

↓

Disconnected
```

The UI updates only by consuming events.

---

# Future Scalability

The architecture should support additional frontends without modifying Core.

Examples:

- Desktop Application
- Command Line Interface
- VSCode Extension
- JetBrains Plugin
- Automated Testing
- Future SDK

All of them should reuse the same Core.

---

# Design Goals

This architecture optimizes for:

- High Performance
- Low Memory Usage
- Long-term Maintainability
- Platform Independence
- Testability
- Reusability
- Clear Separation of Responsibilities

Architecture should remain stable while features evolve.
