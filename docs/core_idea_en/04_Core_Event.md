# Core Event

> Define the event model of the Core.

CoreEvent is the only mechanism used to notify state changes inside the Core.

The Core never updates the UI directly.

The UI observes CoreEvents and decides how to react.

---

# Design Goals

The event system is designed to achieve:

- Loose coupling
- High cohesion
- Asynchronous communication
- Predictable state transitions
- Platform independence
- Testability

Events represent facts.

Events never represent commands.

---

# Core Principles

## Principle 1

Events describe **what has happened**.

Not what should happen.

Correct:

```
ConnectionEstablished
```

Incorrect:

```
ConnectNow
```

---

## Principle 2

Events are immutable.

Once emitted, an event must never be modified.

---

## Principle 3

Events never contain business logic.

They only carry state.

---

## Principle 4

Events belong to domains.

Every domain owns its own events.

No global event list should exist.

---

# Event Hierarchy

```
CoreEvent

├── ApplicationEvent
├── SessionEvent
├── ConnectionEvent
├── ChannelEvent
├── TerminalEvent
├── TransferEvent
├── HostEvent
├── HostKeyEvent
├── CredentialEvent
├── ConfigurationEvent
├── HistoryEvent
└── SystemEvent
```

---

# ApplicationEvent

Represents application lifecycle.

Examples:

- Started
- Ready
- ShutdownRequested
- ShutdownCompleted

---

# SessionEvent

Represents logical session lifecycle.

Examples:

- Created
- Initialized
- Activated
- Closed

---

# ConnectionEvent

Represents SSH transport lifecycle.

Examples:

```
Connecting

TCPConnected

HandshakeStarted

HostKeyVerifying

Authenticated

Ready

Disconnected

ReconnectStarted

ReconnectSucceeded

ReconnectFailed
```

---

# ChannelEvent

Represents SSH channel lifecycle.

Examples:

- Opened
- Ready
- Closed

---

# TerminalEvent

Represents terminal runtime state.

Examples:

- PTYResized
- OutputReceived
- ClipboardRequested
- TitleChanged

---

# TransferEvent

Represents file transfer state.

Examples:

- Started
- Progress
- Paused
- Cancelled
- Finished
- Failed

TransferProgress should contain:

- SessionID
- TransferID
- BytesTransferred
- TotalBytes
- Speed
- ETA

---

# HostEvent

Represents saved host changes.

Examples:

- Created
- Updated
- Deleted
- Imported
- Exported

---

# HostKeyEvent

Represents server identity verification.

Examples:

- Unknown
- Changed
- Accepted
- Rejected

HostKeyChanged should contain:

- HostID
- OldFingerprint
- NewFingerprint

---

# CredentialEvent

Represents credential operations.

Examples:

- Loaded
- Updated
- Deleted
- AccessDenied

Credential values must never appear inside events.

---

# ConfigurationEvent

Represents configuration changes.

Examples:

- Loaded
- Updated
- Saved

---

# HistoryEvent

Represents history updates.

Examples:

- CommandRecorded
- SessionRecorded
- TransferRecorded

---

# SystemEvent

Represents infrastructure changes.

Examples:

- DatabaseOpened
- DatabaseError
- FileSystemError
- NetworkUnavailable

---

# Event Flow

Every operation follows the same pattern.

```
User

↓

IPC Request

↓

Core

↓

Business Logic

↓

State Updated

↓

CoreEvent

↓

IPC Event

↓

UI
```

Events are always emitted after state changes.

---

# Event Ordering

Events must preserve chronological order.

Example:

```
Connecting

↓

TCPConnected

↓

HandshakeStarted

↓

HostKeyVerified

↓

Authenticated

↓

Ready
```

Consumers should never receive these events out of order.

---

# Event Ownership

Each event belongs to exactly one domain.

| Event | Owner |
|---------|-------|
| SessionEvent | Session Domain |
| ConnectionEvent | Connection Domain |
| ChannelEvent | Channel Domain |
| TransferEvent | Transfer Domain |
| HostEvent | Host Domain |
| HostKeyEvent | HostKey Domain |
| CredentialEvent | Credential Domain |
| ConfigurationEvent | Configuration Domain |
| HistoryEvent | History Domain |
| ApplicationEvent | Application Domain |
| SystemEvent | Infrastructure |

Ownership must never overlap.

---

# Event Payload

Every event should contain only the minimum required information.

Events should reference runtime objects by ID.

Example:

```
TransferProgress

SessionID

TransferID

BytesTransferred

TotalBytes
```

Large runtime objects should never be copied into events.

---

# Event Reliability

Events should be:

- Ordered
- Lightweight
- Immutable
- Non-blocking

The event system must never become a bottleneck.

---

# Event Consumers

Events may be consumed by:

- IPC Layer
- Logger
- Metrics
- Future Plugins
- CLI
- Automated Tests

The Core should not know who consumes events.

---

# Event Rules

Events must never:

- Modify state
- Trigger UI directly
- Perform blocking operations
- Own resources

Events are notifications only.

---

# Summary

The Core follows an event-driven architecture.

Every important state transition becomes a domain event.

The Core owns the state.

Events expose the state.

Consumers decide how to react.

This design allows the Core to remain reusable, testable, and independent from any specific frontend.
