# Architecture Decision Record (ADR)

> Record significant architectural decisions made during the project.

Architecture Decision Records preserve the reasoning behind important technical decisions.

The goal is to explain **why** a decision was made, not **how** it was implemented.

---

# ADR Principles

Every ADR should answer four questions:

1. What problem existed?
2. What decision was made?
3. Why was this decision chosen?
4. Which alternatives were rejected?

An ADR is a historical document.

Existing ADRs should never be modified.

If a decision changes, create a new ADR that supersedes the previous one.

---

# ADR-0001

## Title

Core-first architecture.

## Status

Accepted

## Context

The project requires long-term maintainability and possible reuse across different frontends.

## Decision

The Core is developed independently from the UI.

The UI becomes a consumer of Core services.

## Consequences

Benefits:

- Reusable Core
- Easier testing
- Multiple frontend support

Trade-offs:

- Slightly higher initial complexity

Alternatives rejected:

- UI-driven architecture
- Direct frontend-to-library interaction

---

# ADR-0002

## Title

Domain-Oriented Design.

## Status

Accepted

## Context

Folder-based organization becomes difficult to maintain as the project grows.

## Decision

Organize the Core around business domains instead of crate structure.

## Consequences

Benefits:

- Clear ownership
- Better scalability
- Independent testing

Alternatives rejected:

- Utility-oriented modules
- Technology-oriented architecture

---

# ADR-0003

## Title

Event-driven Core.

## Status

Accepted

## Context

SSH operations are asynchronous and state-driven.

The UI should not poll runtime state.

## Decision

Core publishes immutable events.

The UI reacts to events.

## Consequences

Benefits:

- Loose coupling
- Real-time updates
- Better scalability

Alternatives rejected:

- Callback chains
- Polling
- Direct UI updates

---

# ADR-0004

## Title

Separate Session and Connection.

## Status

Accepted

## Context

Logical user sessions and network connections have different lifecycles.

## Decision

Session represents the workspace.

Connection represents the active SSH transport.

## Consequences

Benefits:

- Reconnection becomes simple.
- Runtime state remains stable.
- Future session recovery becomes possible.

Alternatives rejected:

- Session equals Connection

---

# ADR-0005

## Title

One Connection owns multiple Channels.

## Status

Accepted

## Context

SSH protocol naturally supports multiple channels over one transport.

## Decision

Shell, SFTP and future features share a single SSH connection.

## Consequences

Benefits:

- Lower resource usage
- Faster startup
- Better protocol utilization

Alternatives rejected:

- Independent connection per feature

---

# ADR-0006

## Title

SQLite stores only application-specific data.

## Status

Accepted

## Context

Application data differs from SSH ecosystem data.

## Decision

SQLite stores only application-owned information.

## Consequences

Benefits:

- Cleaner separation
- Easier migration
- Simpler storage model

Alternatives rejected:

- Store everything in SQLite

---

# ADR-0007

## Title

Reuse OpenSSH standards.

## Status

Accepted

## Context

OpenSSH already defines portable formats for SSH configuration.

## Decision

Reuse OpenSSH-compatible files whenever possible.

Examples:

- config
- known_hosts
- identity files

## Consequences

Benefits:

- Better compatibility
- Easier migration
- Familiar workflow

Alternatives rejected:

- Proprietary configuration files

---

# ADR-0008

## Title

Credentials belong to the operating system.

## Status

Accepted

## Context

Passwords should never be stored by the application.

## Decision

Credential storage is delegated to the operating system.

## Consequences

Benefits:

- Improved security
- Native integration
- Reduced maintenance

Alternatives rejected:

- SQLite encryption
- Custom credential database

---

# ADR-0009

## Title

Repository and Provider separation.

## Status

Accepted

## Context

Application-owned data differs from externally owned resources.

## Decision

Repositories manage application data.

Providers integrate external systems.

## Consequences

Benefits:

- Clear responsibilities
- Better abstraction
- Easier testing

Alternatives rejected:

- Single persistence abstraction

---

# ADR-0010

## Title

IPC is an adapter.

## Status

Accepted

## Context

The Core should remain frontend-independent.

## Decision

IPC forwards requests and events.

The Core never depends on Tauri.

## Consequences

Benefits:

- Reusable Core
- Future CLI support
- Future plugin support

Alternatives rejected:

- Tauri-dependent Core

---

# ADR-0011

## Title

Service layer between adapters and domains.

## Status

Accepted

## Context

Business workflows often involve multiple domains.

## Decision

Services coordinate domains.

Domains remain focused on business models.

## Consequences

Benefits:

- Clear orchestration
- Better separation of concerns
- Easier future expansion

Alternatives rejected:

- Fat domains
- Logic inside adapters

---

# ADR-0012

## Title

Core Runtime coordinates execution.

## Status

Accepted

## Context

Task scheduling, event dispatching and runtime object management should remain centralized.

## Decision

Introduce Core Runtime as the execution coordinator.

## Consequences

Benefits:

- Cleaner architecture
- Better lifecycle management
- Reduced coupling

Alternatives rejected:

- Multiple independent dispatchers
- Global mutable state

---

# ADR-0013

## Title

User ownership of data.

## Status

Accepted

## Context

Users should never become dependent on proprietary formats.

## Decision

Prefer open standards and platform-native storage.

Application data should remain easy to inspect, migrate and back up.

## Consequences

Benefits:

- Better portability
- Greater transparency
- Long-term trust

Alternatives rejected:

- Closed proprietary formats

---

# ADR-0014

## Title

Performance is an architectural goal.

## Status

Accepted

## Context

The project targets developers who frequently manage multiple SSH sessions.

Responsiveness is a primary requirement.

## Decision

Performance considerations influence architecture from the beginning.

Optimization should occur through good design rather than later patches.

## Consequences

Benefits:

- Lower resource usage
- Better scalability
- Predictable behavior

Alternatives rejected:

- Optimize only after implementation

---

# ADR Template

Future ADRs should follow this template.

```
ADR-XXXX

Title

Status

Context

Decision

Consequences

Alternatives Rejected

Supersedes (Optional)
```

---

# Summary

Architecture evolves.

History should not disappear.

Every significant architectural decision should be documented.

Future contributors should understand not only **what** the architecture is, but also **why** it became that way.
