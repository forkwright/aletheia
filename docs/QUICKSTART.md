# Quickstart

End-to-end guide: from zero to chatting with an agent. Tested on a clean machine. If something here doesn't work, that's a bug in this document -- file an issue.

---

## Prerequisites

| Requirement | Version | Check |
|-------------|---------|-------|
| Rust toolchain | 1.94+ | `rustc --version` |
| Cargo | (ships with Rust) | `cargo --version` |
| Git | any | `git --version` |
| An LLM API key | Anthropic recommended | Have `sk-ant-...` ready |

**System:** 2+ CPU cores, 2 GB RAM minimum (4 GB recommended for the embedding model). Linux (glibc) or macOS 12+.

**Optional:** `pkg-config` and `cmake` are needed only if building the desktop target. The default workspace build (TUI + headless) has no system library dependencies beyond glibc.

Install Rust if you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

---

## 1. Clone and build

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
cargo build --release
```

The binary lands at `target/release/aletheia`. Copy it somewhere on your PATH:

```bash
cp target/release/aletheia ~/.local/bin/
# or: sudo cp target/release/aletheia /usr/local/bin/
```

Verify:

```bash
aletheia --version
```

### Alternative: prebuilt binary

Download the tarball from the [releases page](https://github.com/forkwright/aletheia/releases) instead of building from source:

```bash
VERSION=v0.13.37
curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-linux-x86_64-${VERSION}.tar.gz" \
  -o aletheia.tar.gz
tar xzf aletheia.tar.gz
cd "aletheia-${VERSION}"
sudo cp aletheia /usr/local/bin/
```

---

## 2. Initialize an instance

The init wizard creates the instance directory, writes config, stores your API key, and scaffolds your first agent.

```bash
cd /path/to/where/you/want/instance
aletheia init
```

The wizard prompts for:
- **API provider** (default: Anthropic)
- **API key** (`sk-ant-...`)
- **Model** (default: `claude-sonnet-4-6`)
- **Agent name** (default: Pronoea)
- **Bind address** (default: `localhost`)
- **Auth mode** (default: `none` -- no bearer token required for local use)

For non-interactive setup (CI, scripting, or agent-driven):

```bash
ANTHROPIC_API_KEY=sk-ant-... aletheia init --yes
```

After init completes, you'll have an `instance/` directory:

```text
instance/
├── config/
│   ├── aletheia.toml       # Main config
│   └── credentials/        # API keys (0600 permissions)
├── data/                   # SQLite databases
├── logs/                   # Structured log files
├── nous/
│   └── pronoea/            # Your first agent workspace
├── shared/                 # Runtime infrastructure
├── signal/                 # Signal channel data (optional)
└── theke/                  # Working filesystem
```

---

## 3. Start the server

```bash
aletheia -r ./instance
```

The `-r` flag points to your instance directory. If you run from the directory that contains `instance/`, the server discovers it automatically.

You should see startup logs including the gateway port (default 18789) and your registered agent.

---

## 4. Verify it works

In a second terminal:

```bash
aletheia health
```

Expected output: status `healthy` with your agent listed. If you see `degraded`, your API key wasn't found -- see Troubleshooting below.

```bash
aletheia status
```

Shows agents, sessions, storage, and system info.

---

## 5. Start a conversation

### Option A: Terminal dashboard (TUI)

```bash
aletheia tui
```

Opens a rich terminal UI with markdown rendering, session management, and real-time streaming. Type a message and press Enter.

### Option B: API (curl)

Create a session, then send a message:

```bash
# Create a session
curl -s http://localhost:18789/api/v1/sessions \
  -H "Content-Type: application/json" \
  -H "X-Requested-With: aletheia" \
  -d '{"nous_id": "pronoea"}' | jq .

# Send a message (replace SESSION_ID with the id from above)
curl -N http://localhost:18789/api/v1/sessions/SESSION_ID/messages \
  -H "Content-Type: application/json" \
  -H "X-Requested-With: aletheia" \
  -d '{"content": "Hello, who are you?"}'
```

The messages endpoint streams the response as Server-Sent Events (SSE).

**Note:** If CSRF is enabled in your config (it is by default in the example config), you must include the `X-Requested-With: aletheia` header on all state-changing requests (POST, PUT, DELETE). Read-only GET requests do not require it.

---

## 6. Daily use

```bash
aletheia -r ./instance      # start the server
aletheia tui                 # talk to your agent (in another terminal)
aletheia backup              # create a database backup
aletheia --help              # full command reference
```

Everything runs locally. The embedded knowledge engine, session store, and embedding model are all inside the binary. No external databases, containers, or sidecars.

---

## Optional: systemd service

For always-on operation, install the included systemd user service:

```bash
mkdir -p ~/.config/systemd/user
cp instance.example/services/aletheia.service ~/.config/systemd/user/aletheia.service
```

Edit the service file to match your paths. The default expects the binary at `~/.local/bin/aletheia` and the instance at `~/aletheia/instance`. Key lines to customize:

```ini
EnvironmentFile=-%h/aletheia/instance/config/env
ExecStart=%h/.local/bin/aletheia -r %h/aletheia/instance
```

Then enable and start:

```bash
systemctl --user daemon-reload
systemctl --user enable --now aletheia
loginctl enable-linger    # persist across logout (run once)
journalctl --user -u aletheia -f    # check logs
aletheia health           # verify
```

**Important:** Do not put API keys in the service file (unit files are world-readable). Use the `EnvironmentFile` directive pointing to a file with `0600` permissions, or rely on the credential file that `aletheia init` writes to `instance/config/credentials/`.

---

## Optional: Signal messaging

Talk to your agents over Signal. Requires [signal-cli](https://github.com/AsamK/signal-cli) running as a JSON-RPC daemon.

1. Install and register signal-cli with your phone number
2. Start signal-cli in JSON-RPC mode: `signal-cli -a +15551234567 jsonRpc`
3. Add to `instance/config/aletheia.toml`:

```toml
[channels.signal]
enabled = true

[channels.signal.accounts.default]
account = "+15551234567"
http_host = "localhost"
http_port = 8080

[[bindings]]
channel = "signal"
source = "*"
nous_id = "pronoea"
```

4. Restart the server. Send a message to your Signal number.

See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for DM/group policies, allowlists, and multi-account setup.

---

## Optional: prosoche (heartbeat)

Prosoche is the attention subsystem: a periodic background check where your agent surveys its environment and reports anything needing attention (calendar events, overdue tasks, system health).

Each agent has a `PROSOCHE.md` workspace file defining what to check. The default agent template includes a starter checklist. To configure the heartbeat schedule, see [PROSOCHE.md](PROSOCHE.md).

---

## Upgrade

Replace the binary. Your instance directory, config, and databases are untouched:

```bash
aletheia backup                    # pre-upgrade backup
# Build from source:
git pull && cargo build --release
cp target/release/aletheia ~/.local/bin/
# Or download a new release tarball and copy the binary
aletheia health                    # verify the new version
```

See [UPGRADING.md](UPGRADING.md) for config compatibility, schema migrations, and rollback procedures.

---

## Troubleshooting

### `health` returns `degraded`

No LLM provider credentials found. Check that your API key is available:

```bash
aletheia credential status
```

Fix: either export `ANTHROPIC_API_KEY` in your environment, or re-run `aletheia init` to write the credential file.

### Port already in use

```bash
fuser -k 18789/tcp    # kill the process on that port
# or change the port in instance/config/aletheia.toml:
# [gateway]
# port = 18790
```

### Auth mode `none` with readonly role blocks mutations

The default auth mode from the init wizard is `none` (no bearer token). When `gateway.auth.mode = "none"`, the role assigned to all requests is controlled by `gateway.auth.none_role`. The compiled default is `"admin"`, which permits all operations. If you explicitly set `none_role` to `"readonly"`, only dashboard reads will work -- sessions, messages, and config changes will be rejected.

Fix: ensure your config has:

```toml
[gateway.auth]
mode = "none"
# none_role defaults to "admin" -- do not set it to "readonly" unless intentional
```

### API requests rejected with 403 / missing CSRF header

If CSRF protection is enabled (default in the example config), state-changing requests require the header `X-Requested-With: aletheia`. Add `-H "X-Requested-With: aletheia"` to your curl commands. The TUI handles this automatically.

### Server can't find the instance directory

The server looks for `instance/` in the current directory, or you can specify it explicitly:

```bash
aletheia -r /absolute/path/to/instance
```

You can also set the `ALETHEIA_ROOT` environment variable.

### Signal channel log spam

If signal-cli is not running or not configured, the Signal channel logs connection errors repeatedly. Either start signal-cli, or disable the channel:

```toml
[channels.signal]
enabled = false
```

### Build fails: missing system libraries

The default workspace build has no system library dependencies. If you're building the desktop target (`theatron-desktop`), you need GTK3 and webkit2gtk development libraries. The desktop crate is excluded from the default workspace build for this reason.

---

## Next steps

- [CONFIGURATION.md](CONFIGURATION.md) -- full config reference (models, auth, TLS, rate limiting, sandboxing)
- [DEPLOYMENT.md](DEPLOYMENT.md) -- production setup, TLS, headless build
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) -- extended issue reference
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) -- agent workspace files (SOUL.md, GOALS.md, etc.)
- [ARCHITECTURE.md](ARCHITECTURE.md) -- system architecture and extension points
- [SHELL-COMPLETIONS.md](SHELL-COMPLETIONS.md) -- tab completion for bash, zsh, fish
