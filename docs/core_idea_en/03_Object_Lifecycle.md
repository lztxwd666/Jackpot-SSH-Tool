# Object Lifecycle

> Define the lifecycle and ownership of every runtime object.

This document specifies how objects are created, used and destroyed.

It also defines the relationships between objects.

This document is considered the runtime specification of the Core.

---

# Design Principles

The lifecycle of every object must satisfy:

- Single ownership
- Clear responsibility
- Explicit state transitions
- Predictable destruction
- No hidden side effects

Each object should have a well-defined lifetime.

Objects with different responsibilities must never share the same lifecycle.

---

# Runtime Object Hierarchy

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

Each level owns the level below it.

Destroying a parent destroys all children.

Destroying a child never destroys its parent.

---

# Host Lifecycle

Host represents a saved remote device.

A Host is permanent.

It exists independently from runtime.

Host lifecycle:

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

Deleting a Host permanently removes:

- Connection profile
- User preferences
- Group information
- Associated metadata

Deleting a Host does **not** affect active runtime objects.

Existing Sessions remain valid until closed.

---

# Session Lifecycle

A Session represents one running workspace.

Creating a Session does **not** establish an SSH connection.

Lifecycle:

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

Characteristics:

- Created when user opens a Host.
- Exists regardless of network state.
- Owns runtime state.
- Owns business runtime state. UI runtime state belongs exclusively to the UI layer.
- Can reconnect multiple times.

A Session may create multiple Connections during its lifetime.

A Session owns exactly one active Connection.

---

# Connection Lifecycle

A Connection represents one SSH transport.

Lifecycle:

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

Connection characteristics:

- Exists only while connected.
- Owns SSH transport.
- Owns authentication state.
- Owns SSH channels.
- Never owns Session data.

Destroying a Connection destroys every Channel.

---

# Channel Lifecycle

A Channel represents one SSH communication channel.

Examples:

- Interactive Shell
- SFTP
- Port Forward
- Future Extensions

Lifecycle:

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

Channels always belong to exactly one Connection.

Channels cannot exist independently.

---

# Object Ownership

```
Host
 │
 └──── owns ───► Session
                     │
                     └──── owns ───► Connection
                                            │
                                            └──── owns ───► Channel
```

Ownership is exclusive.

Objects cannot have multiple owners.

---

# Runtime State Ownership

Each object owns only its own state.

## Host owns

- Name
- Address
- Port
- Authentication preference
- User settings
- Metadata

---

## Session owns

- Current status
- Active terminal
- Current working directory (optional)
- UI runtime state
- Runtime cache

---

## Connection owns

- TCP socket
- SSH transport
- Cipher state
- Authentication state
- KeepAlive
- SSH channels

---

## Channel owns

- Channel ID
- Channel type
- Stream
- PTY (Shell)
- SFTP handle (SFTP)

---

# Lifetime Rules

## Rule 1

Host outlives every runtime object.

```
Host

Create once

↓

Reuse many times

↓

Delete manually
```

---

## Rule 2

Session outlives Connection.

```
Session

↓

Connection A

↓

Disconnected

↓

Connection B

↓

Disconnected
```

Reconnect creates a new Connection.

Session remains unchanged.

---

## Rule 3

Connection outlives Channel.

```
Connection

↓

Shell

↓

SFTP

↓

Port Forward
```

Closing Connection closes every Channel.

---

## Rule 4

Channels never reconnect.

Reconnect creates new Channels.

Destroyed Channels are never reused.

---

# Failure Recovery

Failure should always happen at the lowest possible level.

Example:

```
Network Lost

↓

Connection Destroyed

↓

Session survives

↓

Reconnect

↓

New Connection

↓

New Channels
```

The Session should never be destroyed because of a temporary network issue.

---

# Persistence Rules

Only Host is persistent.

Everything else is runtime.

| Object | Persistent |
|----------|------------|
| Host | Yes |
| Session | No |
| Connection | No |
| Channel | No |

Runtime objects must never be written directly into the database.

Only user data is persistent.

---

# Runtime Identity

Each runtime object owns a unique identifier.

Example:

```
HostID

↓

SessionID

↓

ConnectionID

↓

ChannelID
```

Every Event references these identifiers.

Runtime IDs are never reused.

---

# Destruction Rules

Destroying an object always destroys its children.

```
Destroy Host

↓

Destroy Session

↓

Destroy Connection

↓

Destroy Channels
```

Reverse destruction is forbidden.

```
Destroy Channel

×

Destroy Connection
```

Not allowed.

---

# Future Compatibility

This lifecycle supports future features without modification.

Examples:

- SSH Agent
- Port Forwarding
- Multiple Shells
- Background Transfer
- Session Restore
- Plugins

New features should extend existing lifecycles instead of creating parallel ones.

---

# Summary

The runtime model follows a strict hierarchy.

```
Host
    │
Session
    │
Connection
    │
Channel
```

Each layer has:

- Independent lifetime
- Independent responsibility
- Independent state
- Single ownership

This hierarchy forms the foundation of the entire Core architecture.
