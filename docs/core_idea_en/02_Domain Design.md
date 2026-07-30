# Domain Design

> Define the core business domains of the project.

This document describes **what the system is**, not **how it is implemented**.

Implementation details, crate layout, database schema, and IPC interfaces are defined in later documents.

---

# Design Philosophy

The project follows Domain-Oriented Design.

Every module should represent a real concept in the SSH workflow.

A domain should:

- Have a clear responsibility.
- Own its own state.
- Expose a minimal public interface.
- Be independent from UI.
- Be independent from storage implementation.

No domain should exist only because of implementation convenience.

---

# Domain Overview

The project consists of the following core domains.

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

Each domain owns one specific part of the system.

---

# Application Domain

The Application domain represents the entire runtime.

Responsibilities:

- Application lifecycle
- Global state
- Startup
- Shutdown
- Module initialization
- Global event dispatching

The Application domain does not contain SSH logic.

It only coordinates other domains.

---

# Session Domain

A Session represents one user connection entry.

A Session contains:

- Host information
- Authentication method
- User preferences
- Window state
- Runtime status

A Session does **not** own an SSH connection.

A Session is a logical object.

Example:

```
Session

↓

Connect

↓

Create Connection
```

Destroying a Connection does not delete the Session.

---

# Connection Domain

A Connection represents one active SSH transport.

Responsibilities:

- TCP connection
- SSH handshake
- Authentication
- KeepAlive
- Disconnect
- Reconnect (Future)

A Connection exists only while connected.

A Session may create multiple Connections during its lifetime.

---

# Channel Domain

A Channel represents one SSH channel.

Examples:

- Interactive Shell
- SFTP
- Port Forwarding (Future)

A Channel always belongs to one Connection.

A Connection may own multiple Channels.

The lifecycle of a Channel depends on its Connection.

---

# Terminal Domain

The Terminal domain represents terminal state.

Responsibilities:

- PTY size
- Terminal mode
- Input stream
- Output stream
- Encoding
- Clipboard interaction

The Terminal domain does not render terminal content.

Rendering belongs to the UI.

---

# SFTP Domain

Represents the remote filesystem.

Responsibilities:

- Directory browsing
- File metadata
- Upload
- Download
- Rename
- Delete
- Create directory

The SFTP domain never manipulates local UI state.

---

# Transfer Domain

Represents long-running file transfer tasks.

Responsibilities:

- Upload progress
- Download progress
- Speed
- ETA
- Cancellation
- Retry (Future)

Transfer is independent from SFTP browsing.

Browsing directories should remain responsive while transfers continue.

---

# Host Domain

Represents a saved remote machine.

A Host contains:

- Name
- Address
- Port
- Group
- Tags (Future)
- Favorite
- Notes (Future)

A Host is permanent.

It exists even when disconnected.

---

# HostKey Domain

Represents trusted server identities.

Responsibilities:

- Fingerprint verification
- Unknown Host
- HostKey Changed
- Trust decisions
- OpenSSH compatibility

HostKey validation must happen before authentication.

---

# Credential Domain

Represents authentication secrets.

Supported methods:

- Password
- Private Key
- SSH Agent (Future)

Credentials are never stored directly in SQLite.

The Credential domain only communicates with the operating system's secure storage.

---

# Configuration Domain

Represents application configuration.

Examples:

- UI preferences
- SSH defaults
- SFTP defaults
- Transfer preferences

Configuration should remain platform independent whenever possible.

---

# History Domain

Represents local historical records.

Examples:

- Recent sessions
- Connection history
- Command history (Future)
- Transfer history (Future)

History is local.

History must never affect runtime behavior.

---

# Event Domain

Represents communication inside Core.

Every important state transition becomes a CoreEvent.

Examples:

- Connecting
- Connected
- UploadStarted
- UploadProgress
- HostKeyChanged

The Event domain owns no business logic.

It only transports state changes.

---

# Storage Domain

Represents persistent storage.

Responsibilities:

- SQLite
- Data persistence
- Object loading
- Object saving

Storage knows nothing about SSH.

Storage never implements business rules.

---

# Domain Relationships

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

Credential, Storage and Event are shared infrastructure domains used by multiple business domains.

---

# Domain Independence

Each domain should be independently testable.

Example:

Session tests should never require:

- Tauri
- React
- SQLite
- Network

Connection tests should never require:

- UI
- IPC

Storage tests should never require:

- SSH

Independent domains are easier to maintain and reuse.

---

# Ownership Rules

Each runtime object has exactly one owner.

| Object | Owner |
|---------|-------|
| Session | Session Domain |
| Connection | Connection Domain |
| SSH Channel | Channel Domain |
| Terminal State | Terminal Domain |
| Remote Files | SFTP Domain |
| File Transfer | Transfer Domain |
| Saved Host | Host Domain |
| HostKey | HostKey Domain |
| Credential | Credential Domain |
| Configuration | Configuration Domain |
| History | History Domain |
| Events | Event Domain |
| Persistent Data | Storage Domain |

Ownership must never overlap.

---

# Design Goals

This domain model is designed to achieve:

- High cohesion
- Low coupling
- Clear ownership
- Easy testing
- Long-term maintainability
- Reusable Core
- Platform independence

Implementation details should always follow the domain model, never the opposite.
