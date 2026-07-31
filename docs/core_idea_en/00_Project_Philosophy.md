# Project Philosophy

> Build a modern, lightweight, high-performance SSH client focused entirely on the SSH workflow.

---

# Vision

This project aims to build a modern desktop SSH client.

The goal is **not** to become another "all-in-one remote management suite".

Instead, the goal is to provide the best possible SSH experience with excellent performance, intuitive interaction, and a clean architecture.

The project prioritizes maintainability, extensibility, and long-term stability over feature count.

---

# Core Values

## 1. SSH First

SSH is the core of the project.

Everything revolves around the SSH workflow.

Supported features should only exist if they directly improve SSH usage.

Examples include:

- SSH Terminal
- SFTP
- HostKey Management
- Credential Management
- Session Management
- Port Forwarding (Future)
- SSH Config Compatibility

The project intentionally does **not** attempt to become:

- FTP Client
- RDP Client
- VNC Client
- Serial Tool
- Database Client
- Docker Manager
- File Editor
- Remote Desktop Suite

If a feature does not improve the SSH workflow, it does not belong in this project.

---

## 2. Lightweight

Lightweight does **not** mean fewer features.

Lightweight means:

- Fast startup
- Low memory usage
- Low CPU usage
- Minimal runtime dependencies
- Native platform capabilities whenever possible
- No unnecessary abstraction
- No duplicated functionality

Performance is considered a first-class feature.

Every dependency introduced into the project must have a clear justification.

---

## 3. Core First

The Core is the most important part of the project.

The graphical interface is only one possible frontend.

The Core must never depend on:

- React
- Tauri
- xterm.js
- Any GUI framework

The Core should be reusable by:

- Desktop GUI
- CLI
- VSCode Extension
- JetBrains Plugin
- Future Applications

---

## 4. Separation of Responsibilities

Business logic and user interface are completely separated.

Core is responsible for:

- SSH
- SFTP
- Session
- Database
- Credential
- HostKey
- Configuration
- State Management

UI is responsible only for:

- Rendering
- User interaction
- Visual feedback

UI must never implement business logic.

Core must never know the existence of UI.

Communication between them happens only through:

- IPC
- CoreEvent

---

## 5. Event Driven

Core exposes state changes through events.

UI reacts to events.

Core never manipulates the interface directly.

Every important operation should be represented as a state transition.

Examples:

- Connecting
- Connected
- Authentication Started
- Authentication Success
- Upload Progress
- Download Finished
- HostKey Changed

This architecture makes the system easier to extend, debug and test.

---

## 6. Native Platform Integration

Whenever possible, native platform capabilities should be preferred.

Examples:

Passwords:

- Windows Credential Manager
- macOS Keychain
- Linux Secret Service

HostKey:

Compatible with OpenSSH.

SSH Config:

Compatible with OpenSSH.

The project should integrate with existing SSH ecosystems instead of replacing them.

---

## 7. Stability Before Features

A feature is considered complete only when it is:

- Stable
- Maintainable
- Predictable

Adding features is never a goal.

Improving the existing workflow is.

---

## 8. Open Standards First

Whenever an established standard already exists, it should be adopted instead of creating a new one.

Examples:

- OpenSSH Config
- OpenSSH Known Hosts
- SSH Agent Protocol
- Standard SFTP Protocol
- Standard Terminal Escape Sequences

The project should integrate naturally into existing development environments.

Avoid inventing custom formats or proprietary workflows unless absolutely necessary.

---

# Development Principles

Every module should satisfy:

- Single Responsibility Principle
- Clear ownership
- Minimal public API
- Easy testing
- Low coupling
- High cohesion

---

# Non Goals

The following are intentionally outside the scope of this project:

- Remote Desktop
- VNC
- Telnet
- FTP
- Database Client
- Docker Management
- Kubernetes Dashboard
- Text Editor
- IDE Features

These belong to dedicated tools.

---

# Long-Term Goals

The project should become:

- A reusable SSH Core library
- A modern desktop SSH client
- A high-performance alternative to existing commercial SSH clients
- A clean and maintainable open-source project

The architecture should remain understandable after years of continuous development.

Maintainability is considered more important than rapid feature expansion.
