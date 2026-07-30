# UI Design

> Define the design philosophy and interaction principles of the graphical interface.

The UI is responsible only for presentation and user interaction.

Business logic belongs exclusively to the Core.

---

# Design Philosophy

The UI should be:

- Fast
- Responsive
- Predictable
- Minimal
- Modern
- Keyboard-friendly

The interface exists to improve the SSH workflow.

Visual complexity should never replace usability.

---

# Core Principles

## Principle 1

UI never owns business logic.

UI may:

- Display
- Animate
- Filter
- Sort
- Search
- Highlight

UI must never:

- Connect SSH
- Validate HostKey
- Save Database
- Manage Credentials

---

## Principle 2

UI reacts to events.

The interface is a reflection of Core state.

Every visible change should be driven by CoreEvent.

UI should never guess runtime state.

---

## Principle 3

Responsiveness is mandatory.

The interface must remain responsive during:

- Connecting
- Uploading
- Downloading
- Directory Refresh
- Authentication
- Reconnection

Long-running operations must never freeze the interface.

---

## Principle 4

Visual feedback should always exist.

Every user action should receive immediate feedback.

Examples:

Connecting...

Uploading...

Authenticating...

HostKey Verification...

Transfer Progress...

The user should never wonder whether the application is still working.

---

# Layout

The application uses a three-panel layout.

```
+-------------------------------------------------------------+
| Toolbar                                                     |
+-------------+----------------------------+------------------+
|             |                            |                  |
|             |                            |                  |
| Session     |        Terminal            |      SFTP        |
| Tree        |                            |                  |
|             |                            |                  |
|             |                            |                  |
+-------------+----------------------------+------------------+
| Status Bar                                                  |
+-------------------------------------------------------------+
```

The layout should remain clean and distraction-free.

---

# Session Tree

The Session Tree is the primary navigation component.

Responsibilities:

- Host Groups
- Favorites
- Recent Connections
- Search
- Context Menu

The tree should support:

- Expand
- Collapse
- Drag
- Rename
- Keyboard Navigation

---

# Terminal View

The Terminal is the primary workspace.

Responsibilities:

- Terminal Rendering
- Clipboard
- Selection
- Resize
- Keyboard Input

Rendering is delegated to xterm.js.

The Core owns terminal state.

---

# SFTP View

The SFTP panel provides graphical file management.

Recommended layout:

```
Local Files

← Drag →

Remote Files
```

Features:

- Upload
- Download
- Rename
- Delete
- Create Directory
- Refresh

Future features:

- Multi-select
- Background Queue
- Synchronization

---

# Dialogs

Dialogs should interrupt the user only when necessary.

Examples:

- Unknown HostKey
- HostKey Changed
- Delete Confirmation
- Credential Required

Progress dialogs should be non-blocking whenever possible.

---

# Notifications

Notifications should be lightweight.

Examples:

Connected

Upload Finished

Host Saved

Configuration Updated

Errors should provide actionable information.

---

# Search

Search should be available globally.

Future shortcut:

Ctrl + K

Search should locate:

- Hosts
- Groups
- Favorites

Search should never require expanding the tree manually.

---

# Keyboard Navigation

Keyboard usage is a first-class interaction model.

Common operations should have shortcuts.

Examples:

Ctrl + N

New Host

Ctrl + F

Search

Ctrl + W

Close Session

Future shortcuts should remain consistent.

---

# Multiple Sessions

The UI should support multiple simultaneous sessions.

Each session should remain isolated.

Switching between sessions should never reconnect automatically.

---

# Progress Display

Long-running tasks should expose progress.

Examples:

Connection

Authentication

Upload

Download

Directory Loading

Progress should always be driven by CoreEvents.

---

# Theme

The application should support:

- Light Theme
- Dark Theme

Theme selection belongs entirely to the UI.

The Core is unaware of themes.

---

# Window State

The UI owns:

- Window Size
- Window Position
- Panel Size
- Splitter Position
- Active Tab
- Current Focus

The Core must never access these states.

---

# Accessibility

The interface should remain usable through:

- Keyboard
- High DPI Displays
- Screen Scaling

Visual clarity is preferred over decorative effects.

---

# Performance Goals

The interface should prioritize responsiveness.

Recommended goals:

- Fast startup
- Smooth scrolling
- Immediate input response
- Low memory usage
- Stable frame rate

Animation should never reduce usability.

---

# Future Expansion

The UI should support future modules without redesigning the architecture.

Possible additions:

- Port Forward
- SSH Agent
- Background Transfer
- Plugin Panels

New features should integrate naturally into the existing layout.

---

# Summary

The UI is a presentation layer.

Its responsibilities are limited to:

- Rendering
- Interaction
- Feedback

Business logic belongs to the Core.

The UI reflects runtime state through CoreEvents.

A clean interface should improve productivity without increasing complexity.
