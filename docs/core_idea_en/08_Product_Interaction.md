# Product Interaction

> Define the interaction philosophy and workflow of the application.

This document specifies how users interact with the application.

It focuses on interaction consistency rather than visual appearance.

This document is **not** intended to define UI implementation details.

---

# Current Development Status

The project currently prioritizes:

1. Architecture
2. Core Runtime
3. Business Logic
4. Performance
5. Stability

User interface implementation is intentionally simplified during early development.

Interaction optimization will become a primary focus after the Core reaches feature completeness.

---

# Design Philosophy

The application should feel:

- Predictable
- Responsive
- Efficient
- Familiar

The interface should minimize unnecessary user actions.

Every interaction should have a clear purpose.

---

# Core Interaction Principles

## Principle 1

One action.

One result.

Every user action should produce exactly one expected outcome.

Unexpected behavior should never occur.

---

## Principle 2

The interface should never block.

Long-running operations must execute asynchronously.

The UI remains usable while:

- Connecting
- Uploading
- Downloading
- Refreshing directories
- Loading sessions

---

## Principle 3

State should always be visible.

The user should immediately understand:

- What is happening
- What has completed
- What failed
- What requires user input

The interface should never appear frozen.

---

## Principle 4

Destructive operations require confirmation.

Examples:

- Delete Host
- Delete Remote File
- Delete Local File
- Replace Existing File

Confirmation should occur only when necessary.

---

# Startup Experience

The startup sequence should be lightweight.

Preferred flow:

```
Launch

↓

Load Configuration

↓

Load Hosts

↓

Restore UI State

↓

Ready
```

The application should become interactive as early as possible.

---

# Host Management

Hosts are the primary entry point.

Common operations:

- Create
- Edit
- Delete
- Duplicate
- Favorite
- Move Group

Hosts should remain permanently available regardless of connection state.

---

# Connection Workflow

Typical workflow:

```
Double Click Host

↓

Session Created

↓

Connecting

↓

Authentication

↓

Shell Ready
```

Connection progress should always be visible.

---

# HostKey Workflow

Unknown Host:

```
Connect

↓

Unknown HostKey

↓

Display Fingerprint

↓

Trust

↓

Continue
```

Changed HostKey:

```
Connect

↓

HostKey Changed

↓

Display Old Fingerprint

↓

Display New Fingerprint

↓

User Decision

↓

Continue / Abort
```

Users should always understand why the connection is interrupted.

---

# Authentication Workflow

Authentication should require minimal interaction.

Preferred priority:

```
SSH Agent (Future)

↓

Private Key

↓

Password
```

Credential prompts should appear only when necessary.

---

# Terminal Workflow

Opening a session should immediately display the terminal.

The terminal is the primary workspace.

Common actions:

- Input
- Copy
- Paste
- Resize
- Search (Future)

The terminal should never lose focus unexpectedly.

---

# SFTP Workflow

The graphical file manager complements the terminal.

Typical workflow:

```
Browse

↓

Drag

↓

Transfer

↓

Progress

↓

Complete
```

The transfer process should never interrupt browsing.

---

# File Transfer

Transfer operations should always expose:

- Progress
- Speed
- Remaining Time
- Result

Failures should clearly indicate:

- What failed
- Why it failed
- Whether retry is possible

---

# Multiple Sessions

Each session behaves independently.

Changing one session must never affect another.

Closing one session should never disconnect others.

---

# Session Recovery

Future versions may support session recovery.

The expected workflow:

```
Unexpected Disconnect

↓

Reconnect

↓

Restore Terminal

↓

Continue Working
```

Recovery should be automatic whenever possible.

---

# Search

Search should locate:

- Hosts
- Groups
- Favorites

Search should not require manual tree expansion.

Future versions may support fuzzy matching.

---

# Keyboard Interaction

Common operations should remain keyboard accessible.

Examples:

```
Enter

Connect

Delete

Delete Host

F2

Rename

Ctrl + F

Search

Ctrl + W

Close Session
```

Mouse interaction should never be the only option.

---

# Context Menu

Context menus should contain only actions relevant to the selected object.

Example:

Host:

- Connect
- Edit
- Duplicate
- Delete

Remote File:

- Download
- Rename
- Delete

Avoid generic menus containing unrelated actions.

---

# Error Presentation

Errors should explain:

- What happened
- Why it happened
- What the user can do next

Example:

Instead of:

```
Connection Failed
```

Prefer:

```
Authentication failed.

Please verify your username, password or private key.
```

Actionable information is preferred over technical details.

---

# Notification Strategy

Notifications should be lightweight.

Examples:

- Connected
- Upload Complete
- Download Complete
- Host Saved

Long-running operations should use progress indicators instead of repeated notifications.

---

# Consistency

Similar operations should behave identically throughout the application.

Examples:

Double-click always opens.

Delete always requires confirmation.

Rename always uses the same interaction.

Consistency is more important than novelty.

---

# Future Improvements

The following are intentionally postponed until after Core development:

- Advanced animations
- UI polish
- Theme customization
- Layout optimization
- Rich drag-and-drop effects
- Accessibility enhancements
- Fine-grained interaction improvements

These improvements should not require changes to the Core architecture.

---

# Summary

The interaction model follows one simple principle:

```
User Action

↓

Core Request

↓

Core Event

↓

UI Feedback
```

The user should always understand:

- What they requested.
- What the application is doing.
- What the final result is.

Interaction quality should improve productivity without increasing complexity.
