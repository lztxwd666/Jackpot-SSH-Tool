# Persistent Storage

> Define how user data is persisted while remaining fully compatible with existing SSH ecosystems.

Persistent Storage is an infrastructure service.

Its only responsibility is preserving user-owned data.

Business logic never depends on storage implementation.

---

# Design Philosophy

Persistent storage exists to preserve user data.

It is **not** responsible for:

- Business logic
- Runtime state
- Session lifecycle
- SSH protocol
- Event dispatching

Storage is passive.

The Core owns the data.

Storage only persists it.

---

# Open Standards First

Whenever an established standard already exists, it should be adopted instead of creating a proprietary format.

The project should integrate naturally into the existing OpenSSH ecosystem.

User data should remain readable, portable and reusable outside this application.

---

# Storage Architecture

```
Business Domain

↓

Repository / Provider

↓

Storage Layer

↓

SQLite
OpenSSH Files
Operating System Services
```

Business domains never access storage directly.

Every persistent operation goes through a dedicated abstraction.

---

# Persistent Data Classification

Persistent data is divided into three categories.

```
SQLite
OpenSSH Files
Operating System Services
```

Each category owns different responsibilities.

---

# SQLite

SQLite stores application-specific data.

Examples include:

- Saved Hosts
- Groups
- Favorites
- Application Configuration
- Recent Connections
- Transfer History
- Local Command History (Future)
- UI Preferences

SQLite should never store security-sensitive secrets.

---

# OpenSSH Files

Whenever possible, existing OpenSSH files should be reused instead of introducing new formats.

Examples:

```
~/.ssh/config

~/.ssh/known_hosts

~/.ssh/id_rsa

~/.ssh/id_ed25519

~/.ssh/authorized_keys
```

On Windows, the equivalent OpenSSH directory should be used.

The application should remain compatible with files created by OpenSSH.

Likewise, files modified by this application should remain usable by OpenSSH.

---

# Operating System Services

Sensitive information belongs to the operating system.

Examples:

Passwords

Private key passphrases

Tokens (Future)

Examples:

Windows

Credential Manager

Linux

Secret Service

macOS

Keychain

Sensitive information must never be stored inside SQLite.

---

# Repository / Provider Responsibilities

Persistent operations are exposed through dedicated abstractions.

Examples:

```
HostRepository

ConfigurationRepository

HistoryRepository

TransferHistoryRepository

SSHConfigProvider

KnownHostsProvider

CredentialProvider
```

Repositories own application data.

Providers integrate external systems.

---

# Repository vs Provider

Repositories own data managed by the application.

Examples:

```
HostRepository

HistoryRepository

ConfigurationRepository
```

Providers expose external resources owned by the operating system or existing standards.

Examples:

```
CredentialProvider

KnownHostsProvider

SSHConfigProvider
```

This distinction keeps business logic independent from infrastructure.

---

# Object Ownership

Each persistent object has exactly one owner.

| Object | Owner |
|----------|-------|
| Saved Host | HostRepository |
| Groups | HostRepository |
| Favorites | HostRepository |
| Configuration | ConfigurationRepository |
| History | HistoryRepository |
| Transfer History | TransferHistoryRepository |
| SSH Config | SSHConfigProvider |
| Known Hosts | KnownHostsProvider |
| Credentials | CredentialProvider |

Ownership must never overlap.

---

# Runtime Objects

Runtime objects are never persisted.

Examples:

```
Session

Connection

Channel

Transfer Task

Runtime Cache

Events

Network State
```

Runtime objects disappear when the application exits.

---

# Transactions

Repositories should perform atomic updates whenever multiple persistent objects are modified.

Example:

```
Create Host

↓

Save Host

↓

Save Group Mapping

↓

Commit
```

Partial persistence should never occur.

---

# Identity

Persistent objects own stable identifiers.

Examples:

```
HostID

GroupID

HistoryID

TransferHistoryID
```

Identifiers never change.

Relationships should always reference identifiers instead of duplicating objects.

---

# Caching

Repositories may cache frequently accessed data.

Caching is an implementation detail.

Business logic must never rely on cache existence.

---

# Migration

SQLite should support automatic schema migration.

Migration requirements:

- Schema Version
- Automatic Upgrade
- Backward Compatibility whenever possible

OpenSSH files should never require migration.

They follow the OpenSSH specification.

---

# Backup Strategy

The application should remain easy to back up.

Recommended directory layout:

```
Application/

├── config.db
├── logs/
├── cache/
└── temp/
```

SSH-related files remain inside the standard OpenSSH directory.

Restoring both directories restores the complete application.

---

# Performance Principles

Persistent operations should:

- Avoid unnecessary disk writes
- Batch updates when appropriate
- Minimize I/O
- Execute asynchronously whenever possible

Storage should never block the UI.

---

# Error Handling

Infrastructure-specific errors should never leak into business domains.

Example:

Instead of:

```
SQLite Error
```

Expose:

```
SaveHostFailed
```

Instead of:

```
Permission Denied

known_hosts
```

Expose:

```
HostKeyUpdateFailed
```

Infrastructure details remain inside repositories and providers.

---

# Future Compatibility

The storage architecture should support future expansion without changing business domains.

Possible future implementations:

- SQLite
- In-memory Storage
- JSON Storage (Testing)
- PostgreSQL (Future)

The Core should remain independent from the storage backend.

---

# Summary

Persistent storage is composed of three independent layers.

```
Application Data
        │
        ▼
SQLite
```

```
SSH Ecosystem
        │
        ▼
OpenSSH Files
```

```
Sensitive Information
        │
        ▼
Operating System Services
```

Application-specific information belongs to SQLite.

OpenSSH-compatible information belongs to OpenSSH.

Sensitive information belongs to the operating system.

The project should integrate with existing standards whenever possible, instead of replacing them.
