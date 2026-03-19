# Quickstart

## Install

Download the prebuilt binary for your platform:

```bash
# Linux x86_64
curl -L https://github.com/forkwright/aletheia/releases/latest/download/aletheia-linux-x86_64 -o aletheia
chmod +x aletheia && sudo mv aletheia /usr/local/bin/

# macOS Apple Silicon
curl -L https://github.com/forkwright/aletheia/releases/latest/download/aletheia-macos-aarch64 -o aletheia
chmod +x aletheia && sudo mv aletheia /usr/local/bin/
```

Or build from source (requires Rust 1.94+):

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cargo build --release
cp target/release/aletheia ~/.local/bin/
```

## Setup

The init wizard creates an instance directory, writes config, sets up credentials, and scaffolds your first agent:

```bash
aletheia init
```

It will prompt for your API key, model preference, and agent name. For non-interactive setup:

```bash
ANTHROPIC_API_KEY=sk-ant-... aletheia init --yes
```

## Run

```bash
aletheia              # start the server
aletheia tui          # open the terminal dashboard
```

In another terminal:

```bash
aletheia health       # verify the server is running
aletheia status       # agent status, sessions, storage
```

## Daily use

```bash
aletheia              # start the server
aletheia tui          # talk to your agent
aletheia backup       # create a database backup
aletheia --help       # full command reference
```

Everything runs locally. The embedded knowledge engine, session store, and embedding model are all inside the binary. No external databases, containers, or sidecars.

## Optional: Signal messaging

Talk to your agents over Signal. Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for setup.

## Next steps

- [CONFIGURATION.md](CONFIGURATION.md): full config reference
- [DEPLOYMENT.md](DEPLOYMENT.md): production setup, systemd, TLS
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md): common issues and fixes
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md): agent workspace files
- [ARCHITECTURE.md](ARCHITECTURE.md): system architecture and extension points
