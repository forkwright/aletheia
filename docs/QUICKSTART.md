# Quickstart

## Prerequisites

- Rust toolchain (stable)
- An Anthropic API key (or Claude Code installed - key is auto-detected)

## Install

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
cargo build --release
cp target/release/aletheia ~/.local/bin/
```

## Daily Use

```bash
aletheia              # start the server (gateway, agents, daemon)
aletheia health       # check config and connectivity
aletheia status       # agent status, sessions, cron jobs
aletheia backup       # create a point-in-time database backup
```

Run `aletheia --help` for the full command reference.

Memory (CozoDB, fastembed-rs) is embedded in the binary. No external databases, containers, or sidecars required.

## Optional: Signal Integration

Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for setup.

## Next Steps

- [CONFIGURATION.md](CONFIGURATION.md) - config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) - production setup
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) - agent workspace files
- [ARCHITECTURE.md](ARCHITECTURE.md) - system architecture and extension points
