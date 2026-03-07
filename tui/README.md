# Aletheia TUI

Terminal interface for Aletheia, built with Rust/Ratatui.

## Setup

The TUI connects to a running Aletheia gateway. Start the gateway first:

```bash
aletheia start
```

## Build

```bash
cargo build --release
```

## Run

```bash
./target/release/aletheia-tui
# or during development:
cargo run
```

Configuration is read from `instance/config/aletheia.yaml`. The TUI connects to the gateway URL defined in that file (default `http://localhost:18789`).

## Configuration

The TUI respects the same `ALETHEIA_CONFIG_DIR` environment variable as the gateway:

```bash
ALETHEIA_CONFIG_DIR=~/.aletheia cargo run
```

## Credential setup

Credentials are shared with the gateway. To set up credentials manually:

```bash
mkdir -p ~/.aletheia/credentials
echo '{"apiKey": "sk-ant-..."}' > ~/.aletheia/credentials/anthropic.json
chmod 600 ~/.aletheia/credentials/anthropic.json
```
