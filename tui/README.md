# Aletheia TUI

Terminal interface for Aletheia, built with Rust/Ratatui.

## Setup

The TUI connects to a running Aletheia gateway. Start the gateway first:

```bash
cd ..
./setup.sh        # first run: builds everything, starts gateway
# or manually:
ALETHEIA_ROOT=$(pwd) node infrastructure/runtime/dist/entry.mjs
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

Configuration is read from `~/.aletheia/aletheia.json`. The TUI connects to the gateway URL defined in that file (default `http://localhost:18789`).

## Configuration

The TUI respects the same `ALETHEIA_CONFIG_DIR` environment variable as the gateway:

```bash
ALETHEIA_CONFIG_DIR=~/.aletheia cargo run
```

## Credential setup

Credentials are shared with the gateway — if you've run `./setup.sh` and completed the web wizard, the TUI will work automatically. To set up credentials manually:

```bash
# Create the credentials directory and file:
mkdir -p ~/.aletheia/credentials
echo '{"apiKey": "sk-ant-..."}' > ~/.aletheia/credentials/anthropic.json
chmod 600 ~/.aletheia/credentials/anthropic.json
```
