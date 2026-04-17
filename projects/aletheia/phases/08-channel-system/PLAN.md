# Phase 08: Channel system

## Goal
Agents can communicate over multiple channels including Signal messenger.

## Success criteria
- Channel provider trait allows registration of new channels without core changes
- Signal provider delivers and receives messages with < 5s latency
- Message format supports markdown, code blocks, and file attachments
- Channel health check fails within 30s of provider outage

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Channel provider trait allows registration of new channels without core changes | Adding a new channel requires modifying agora crate internals |
| Signal provider delivers and receives messages with < 5s latency | End-to-end test shows message round-trip >= 5s |
| Message format supports markdown, code blocks, and file attachments | Rendering test shows malformed markdown or broken attachments |
| Channel health check fails within 30s of provider outage | Health check continues to pass 60s after signal-cli process killed |

## Scope

### In scope
- agora crate: channel registry, ChannelProvider trait
- semeion crate: Signal provider via signal-cli subprocess
- Message formatting and parsing

### Out of scope
- Matrix provider (separate feature, deferred)
- Email provider
- Voice/video channels

## Requirements
- REQ-01: signal-cli subprocess is managed with automatic restart
- REQ-02: Message history is persisted to session store
- REQ-03: File attachments are saved to instance directory with size limits
- REQ-04: Channel commands (15 built-in) are documented in operator guide

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Signal integration | signal-cli subprocess over libsignal | Avoids linking complex Rust crate, easier debugging |
| Subprocess management | tokio::process with watchdog | Automatic restart on crash |

## Open questions
- Should we support group chats? (Resolved: yes, via Signal groups)

## Dependencies
- Phase 07 complete
- signal-cli installed and linked to phone number
