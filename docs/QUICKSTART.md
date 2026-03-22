# Quickstart

## Install

Download the prebuilt tarball for your platform. Replace `VERSION` with the latest tag from the [releases page](https://github.com/forkwright/aletheia/releases):

```bash
# Linux x86_64
VERSION=v0.13.0
curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-linux-x86_64-${VERSION}.tar.gz" \
  -o aletheia.tar.gz
sha256sum -c "aletheia-linux-x86_64-${VERSION}.tar.gz.sha256"
tar xzf aletheia.tar.gz
cd "aletheia-${VERSION}"
sudo cp aletheia /usr/local/bin/

# macOS Apple Silicon
VERSION=v0.13.0
curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-macos-aarch64-${VERSION}.tar.gz" \
  -o aletheia.tar.gz
shasum -a 256 -c "aletheia-macos-aarch64-${VERSION}.tar.gz.sha256"
tar xzf aletheia.tar.gz
cd "aletheia-${VERSION}"
sudo cp aletheia /usr/local/bin/
```

The tarball extracts to `aletheia-${VERSION}/` and contains:
- `aletheia` — the binary
- `instance.example/` — reference config and directory layout for first-time setup

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

## Upgrade

To upgrade an existing installation, replace the binary. Your instance directory, config, and database are untouched:

```bash
aletheia backup          # create a pre-upgrade backup
VERSION=v0.13.0
curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-linux-x86_64-${VERSION}.tar.gz" \
  -o aletheia.tar.gz
tar xzf aletheia.tar.gz
sudo cp "aletheia-${VERSION}/aletheia" /usr/local/bin/aletheia
aletheia health          # verify the new version is running
```

See [UPGRADING.md](UPGRADING.md) for config compatibility, schema migrations, and rollback procedures.

## Optional: Signal messaging

Talk to your agents over Signal. Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for setup.

## Next steps

- [CONFIGURATION.md](CONFIGURATION.md): full config reference
- [DEPLOYMENT.md](DEPLOYMENT.md): production setup, systemd, TLS
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md): common issues and fixes
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md): agent workspace files
- [ARCHITECTURE.md](ARCHITECTURE.md): system architecture and extension points
