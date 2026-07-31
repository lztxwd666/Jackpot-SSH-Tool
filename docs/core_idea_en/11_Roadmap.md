# Roadmap

> Define the long-term implementation plan of the project.

This roadmap is organized around architectural capabilities rather than individual features.

Each stage should produce a usable milestone.

Earlier stages provide the foundation for later stages.

---

# Stage 0 — Architecture

## Goal

Complete software architecture design.

## Deliverables

- Project Philosophy
- Architecture
- Domain Design
- Lifecycle
- Core Event
- Persistent Storage
- Core API & IPC
- UI Design
- Product Interaction
- Engineering Guidelines
- Architecture Decision Records
- Roadmap

## Milestone

The project architecture is fully defined.

No production code has been written.

---

# Stage 1 — Foundation

## Goal

Build the project skeleton.

## Deliverables

Workspace structure

Cargo workspace

Logging system

Configuration loading

Error handling

Core Runtime

Service layer

Repository interfaces

Provider interfaces

CoreEvent framework

SQLite initialization

Dependency injection

Unit test framework

## Milestone

The application starts successfully.

Core Runtime can execute.

Events can be dispatched.

Nothing SSH-related exists yet.

---

# Stage 2 — SSH Core

## Goal

Implement the SSH runtime.

## Deliverables

ssh2-rs integration

Connection Service

Session lifecycle

Connection lifecycle

Authentication

HostKey verification

KnownHosts Provider

Credential Provider

KeepAlive

Reconnect framework

Channel management

## Milestone

The application can reliably establish SSH connections.

Terminal UI is not yet available.

---

# Stage 3 — Host Management

## Goal

Implement persistent host management.

## Deliverables

Host Repository

Group management

Favorites

Search

CRUD operations

SQLite persistence

Configuration persistence

## Milestone

Hosts can be created, edited, deleted and stored permanently.

No SSH connection yet.

---

# Stage 4 — Terminal

## Goal

Create a fully functional terminal experience.

## Deliverables

xterm.js integration

PTY management

Input forwarding

Output rendering

Resize handling

Clipboard support

Multiple sessions

Terminal tabs

## Milestone

The application becomes a usable SSH client.

Users can work entirely from the terminal.

---

# Stage 5 — SFTP

## Goal

Implement graphical file transfer.

## Deliverables

SFTP channel

Remote browser

Local browser

Drag and drop

Upload

Download

Rename

Delete

Create directory

Transfer queue

Progress reporting

Cancellation

## Milestone

The application provides a complete graphical SSH workflow.

---

# Stage 6 — Product Completion

## Goal

Improve usability and overall experience.

## Deliverables

Session tree improvements

Search

Keyboard shortcuts

History

UI optimization

Theme support

Performance tuning

Accessibility

Settings

Error messages

Interaction refinement

## Milestone

The application becomes comfortable for daily development work.

---

# Stage 7 — Release Preparation

## Goal

Prepare the project for public release.

## Deliverables

Documentation

Benchmarks

Stress testing

Memory profiling

Performance optimization

Packaging

Installer

CI/CD

Versioning

Release notes

## Milestone

Version 1.0 is ready for public use.

---

# Future Features

The following features are intentionally postponed.

Port Forward

SSH Agent

Command History Synchronization

Session Recovery

Plugin System

Cloud Sync

Script Automation

AI Assistance

Additional Authentication Methods

These features should extend the existing architecture without requiring major redesign.

---

# Non-Goals (Version 1.x)

The following are intentionally excluded.

FTP

Telnet

Serial Console

RDP

VNC

Docker Management

Kubernetes Management

Database Clients

File Editors

Process Managers

Terminal Multiplexers

The application remains focused on SSH.

---

# Development Priorities

The implementation order always follows this sequence.

```
Architecture

↓

Core Runtime

↓

Business Logic

↓

Persistence

↓

SSH Runtime

↓

Terminal

↓

SFTP

↓

UI Polish
```

Visual improvements should never delay Core development.

---

# Success Criteria

Version 1.0 should satisfy the following objectives.

Reliable SSH connections

Stable terminal experience

Responsive SFTP operations

Low memory usage

Fast startup

Simple configuration

OpenSSH compatibility

Platform-native credential management

Clear architecture

Comprehensive documentation

---

# Long-Term Vision

The project aims to become a modern, lightweight and high-performance SSH client.

Core principles remain unchanged.

- Architecture before implementation.
- Performance before complexity.
- Open standards before proprietary formats.
- User ownership before vendor lock-in.
- Simplicity before feature accumulation.

Future development should strengthen these principles rather than compromise them.
