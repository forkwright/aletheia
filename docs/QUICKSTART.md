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

## Initialize

```bash
export ANTHROPIC_API_KEY=sk-ant-...   # or enter it interactively
aletheia init                          # creates instance/, prompts for settings
```

Accepts `--yes` for non-interactive setup (uses defaults + env var API key):

```bash
ANTHROPIC_API_KEY=sk-ant-... aletheia init --yes
```

## Run

```bash
aletheia              # start the server (gateway, agents, daemon)
aletheia health       # check connectivity — prints OK or FAILED
aletheia tui          # launch the terminal dashboard
```

## Daily use

```bash
aletheia              # start the server (gateway, agents, daemon)
aletheia health       # check config and connectivity
aletheia status       # agent status, sessions, cron jobs
aletheia backup       # create a point-in-time database backup
```

Run `aletheia --help` for the full command reference.

Memory (embedded engine, candle) is embedded in the binary. No external databases, containers, or sidecars required.

## Optional: Signal integration

Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for setup.

## Next steps

- [CONFIGURATION.md](CONFIGURATION.md) - config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) - production setup
- [troubleshooting.md](troubleshooting.md) - common issues and fixes
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) - agent workspace files
- [ARCHITECTURE.md](ARCHITECTURE.md) - system architecture and extension points
