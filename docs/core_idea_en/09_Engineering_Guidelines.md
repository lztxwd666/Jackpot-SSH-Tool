# Engineering Guidelines

> Define the engineering rules that every contributor must follow.

These guidelines ensure long-term maintainability, consistency, and architectural integrity.

Every implementation should follow these rules unless there is a strong, documented reason not to.

---

# Philosophy

Architecture is more important than implementation.

Implementation can change.

Architecture should remain stable.

When implementation conflicts with architecture, implementation must change.

---

# Core Principles

## Principle 1

Respect domain boundaries.

Every feature belongs to exactly one domain.

Never create "utility domains" or "miscellaneous modules" to avoid proper design.

---

## Principle 2

Dependencies always point downward.

```
UI
    ↓
Adapter
    ↓
Service
    ↓
Domain
    ↓
Infrastructure
```

Lower layers must never depend on higher layers.

---

## Principle 3

Business logic belongs only to the Core.

Never implement business rules in:

- UI
- IPC
- Repository
- Database
- Event Dispatcher

---

## Principle 4

Every module should have one responsibility.

If a module performs unrelated work, split it.

High cohesion is preferred over convenience.

---

# Domain Rules

A new Domain should be introduced only if:

- It represents a real business concept.
- It has an independent lifecycle.
- It owns independent state.
- It has clear responsibilities.

Do not create Domains merely to organize files.

---

# Service Rules

Services coordinate business behavior.

Services may:

- Call multiple domains.
- Validate business rules.
- Emit events.

Services must not:

- Access UI.
- Access IPC.
- Access SQLite directly.
- Store global mutable state.

---

# Repository Rules

Repositories own persistence.

Repositories may:

- Load data.
- Save data.
- Query data.

Repositories must not:

- Validate business rules.
- Emit UI events.
- Open network connections.
- Know SSH protocol.

---

# Provider Rules

Providers integrate with external systems.

Examples:

- Credential Provider
- KnownHosts Provider
- SSHConfig Provider

Providers should expose a clean interface independent of platform-specific details.

---

# Event Rules

Every event represents something that has already happened.

Events must:

- Be immutable.
- Be lightweight.
- Contain only required information.

Events must never:

- Execute logic.
- Trigger side effects.
- Modify runtime state.

---

# API Rules

Every public API belongs to one Service.

APIs should:

- Perform one action.
- Return immediately whenever possible.
- Report long-running state through events.

Avoid large "manager" APIs containing unrelated methods.

---

# Runtime Rules

The Runtime coordinates execution.

The Runtime may:

- Schedule tasks.
- Dispatch events.
- Own object registries.

The Runtime must never implement business logic.

---

# UI Rules

The UI is a consumer of state.

The UI may:

- Render.
- Filter.
- Sort.
- Search.
- Animate.

The UI must never:

- Own business state.
- Validate SSH.
- Access SQLite.
- Manage credentials.

---

# IPC Rules

IPC is an adapter.

IPC should:

- Translate requests.
- Forward events.

IPC must never:

- Implement business logic.
- Modify events.
- Maintain application state.

---

# Error Handling

Errors should be layered.

Infrastructure errors remain inside infrastructure.

Business errors are exposed to Services.

UI receives user-oriented errors.

Avoid exposing implementation details.

---

# Dependency Rules

Avoid unnecessary dependencies.

Before introducing a new dependency, evaluate:

- Is it actively maintained?
- Is it mature?
- Is it widely used?
- Is it performant?
- Can the standard library solve this instead?

Prefer fewer dependencies.

---

# Async Rules

Async should be used only when necessary.

Avoid asynchronous code for purely synchronous operations.

Every spawned task should have:

- Clear ownership.
- Defined lifetime.
- Cancellation strategy.

Detached tasks should be avoided unless justified.

---

# Shared State

Shared mutable state should be minimized.

Prefer:

- Ownership
- Message passing
- Immutable data

Use synchronization primitives only when ownership is impossible.

---

# Locking Rules

Avoid nested locks.

Prefer short lock durations.

Never hold locks across network operations.

Never hold locks while awaiting asynchronous operations.

---

# Event Naming

Events describe completed actions.

Preferred:

```
ConnectionEstablished

TransferStarted

HostCreated
```

Avoid imperative names:

```
Connect

Upload

CreateHost
```

---

# Identifier Rules

Persistent objects own stable IDs.

Runtime objects own runtime IDs.

Identifiers should never be reused.

Avoid exposing internal pointers.

---

# Testing

Every domain should be independently testable.

Tests should avoid:

- UI
- IPC
- SQLite
- Network

Whenever possible, mock repositories and providers instead of real implementations.

---

# Logging

Logging should aid debugging without becoming noise.

Recommended levels:

- TRACE — Detailed execution flow.
- DEBUG — Internal state changes.
- INFO — Important lifecycle events.
- WARN — Recoverable issues.
- ERROR — Operation failures.

Avoid excessive logging in hot paths.

---

# Performance

Performance is a design goal.

Every feature should consider:

- CPU usage
- Memory usage
- Allocation count
- Lock contention
- Startup time

Measure performance before optimizing.

Avoid premature optimization.

---

# Code Review Checklist

Before merging changes, verify:

- Domain boundaries remain intact.
- Responsibilities are clear.
- No architectural violations exist.
- Tests are sufficient.
- Public APIs remain consistent.
- Documentation is updated when necessary.

---

# Documentation

Architecture changes must update documentation first.

Implementation should follow documentation.

Documentation is part of the project, not an afterthought.

---

# Future Compatibility

New features should extend the existing architecture.

Avoid introducing parallel implementations.

When unsure, prefer consistency over novelty.

---

# Summary

The project follows one fundamental rule:

```
Architecture

↓

Engineering Rules

↓

Implementation
```

Code should evolve.

Architecture should endure.

Every contribution should strengthen the architecture rather than weaken it.
