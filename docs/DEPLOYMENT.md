# Deployment

Production operations guide for Aletheia. For first-time setup, see [QUICKSTART.md](QUICKSTART.md).

## Boot Persistence

### macOS (launchd)

#### Prerequisites

- Node.js 22+ in PATH (verify: `node --version`)
- Homebrew (for memory services): https://brew.sh
- Build artifacts present: `infrastructure/runtime/dist/entry.mjs` must exist

#### Enable / Disable

```bash
aletheia enable    # installs plists to ~/Library/LaunchAgents/, registers with launchd, starts at login
aletheia disable   # unloads services, removes plist files
```

`aletheia enable` is idempotent — safe to re-run (e.g., after `aletheia update` or a Node.js upgrade).

#### What `aletheia enable` installs

Two plist files are installed to `~/Library/LaunchAgents/` with real paths substituted for tokens at install time.

**Token substitution:**

| Token | Resolved value |
|-------|---------------|
| `__NODE_BIN__` | `$(which node)` — captured at enable time from current PATH |
| `__ALETHEIA_HOME__` | Repo root (resolved via `realpath`) |
| `__ALETHEIA_CONFIG_DIR__` | `~/.aletheia` (or `$ALETHEIA_CONFIG_DIR` if set) |
| `__PATH__` | `/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin` |

The log directory `~/.aletheia/logs/` is created automatically by `aletheia enable`.

**`com.aletheia.gateway.plist` (template):**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.aletheia.gateway</string>

  <key>ProgramArguments</key>
  <array>
    <string>__NODE_BIN__</string>
    <string>__ALETHEIA_HOME__/infrastructure/runtime/dist/entry.mjs</string>
    <string>gateway</string>
    <string>start</string>
  </array>

  <key>RunAtLoad</key>
  <true/>

  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>

  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>__PATH__</string>
    <key>ALETHEIA_HOME</key>
    <string>__ALETHEIA_HOME__</string>
    <key>ALETHEIA_CONFIG_DIR</key>
    <string>__ALETHEIA_CONFIG_DIR__</string>
  </dict>

  <key>WorkingDirectory</key>
  <string>__ALETHEIA_HOME__</string>

  <key>StandardOutPath</key>
  <string>__ALETHEIA_CONFIG_DIR__/logs/gateway.log</string>

  <key>StandardErrorPath</key>
  <string>__ALETHEIA_CONFIG_DIR__/logs/gateway.log</string>
</dict>
</plist>
```

**`com.aletheia.memory.plist` (template):**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.aletheia.memory</string>

  <key>ProgramArguments</key>
  <array>
    <string>__ALETHEIA_HOME__/bin/aletheia</string>
    <string>start</string>
    <string>--memory-only</string>
  </array>

  <key>RunAtLoad</key>
  <true/>

  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>

  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>__PATH__</string>
    <key>ALETHEIA_HOME</key>
    <string>__ALETHEIA_HOME__</string>
  </dict>

  <key>WorkingDirectory</key>
  <string>__ALETHEIA_HOME__</string>

  <key>StandardOutPath</key>
  <string>__ALETHEIA_CONFIG_DIR__/logs/memory.log</string>

  <key>StandardErrorPath</key>
  <string>__ALETHEIA_CONFIG_DIR__/logs/memory.log</string>
</dict>
</plist>
```

#### Manual launchctl commands

```bash
# Load (register with launchd + start immediately)
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.aletheia.gateway.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.aletheia.memory.plist

# Unload — uses label form, NOT file path
launchctl bootout gui/$(id -u)/com.aletheia.gateway
launchctl bootout gui/$(id -u)/com.aletheia.memory

# Status
launchctl print gui/$(id -u)/com.aletheia.gateway
launchctl print gui/$(id -u)/com.aletheia.memory
```

> **Note:** `launchctl bootout` requires the service label (`gui/UID/com.aletheia.gateway`), not a file path. Using a file path is a common mistake and will fail.

#### Log files

```
Gateway: ~/.aletheia/logs/gateway.log
Memory:  ~/.aletheia/logs/memory.log
```

`aletheia enable` creates `~/.aletheia/logs/` automatically before installing the plists.

```bash
aletheia logs -f                          # follow gateway log via aletheia CLI
tail -f ~/.aletheia/logs/gateway.log      # follow directly
tail -f ~/.aletheia/logs/memory.log
```

---

### Linux (systemd)

#### Enable / Disable

```bash
aletheia enable    # installs unit files to ~/.config/systemd/user/, enables for boot
aletheia disable   # disables services, removes unit files
```

#### What `aletheia enable` installs

Two unit files are installed to `~/.config/systemd/user/` with real paths substituted for tokens. The same token table applies as for macOS, plus `__COMPOSE_CMD__` (resolved to `docker compose` or `podman compose`).

**`aletheia.service` (template):**

```ini
[Unit]
Description=Aletheia AI Gateway
After=network-online.target
Wants=network-online.target aletheia-memory.service

[Service]
Type=simple
Environment=ALETHEIA_HOME=__ALETHEIA_HOME__
EnvironmentFile=-%h/.aletheia/env
ExecStart=__NODE_BIN__ __ALETHEIA_HOME__/infrastructure/runtime/dist/entry.mjs gateway start
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
```

**`aletheia-memory.service` (template):**

```ini
[Unit]
Description=Aletheia Memory Sidecar (Qdrant + Neo4j + Mem0)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=__ALETHEIA_HOME__/infrastructure/memory/sidecar
Environment=NEO4J_PASSWORD=chiron-memory
Environment=NEO4J_AUTH=neo4j/chiron-memory
EnvironmentFile=-%h/.aletheia/env
ExecStartPre=__COMPOSE_CMD__ \
  --project-directory __ALETHEIA_HOME__/infrastructure/memory \
  -p memory up -d
ExecStart=__ALETHEIA_HOME__/infrastructure/memory/sidecar/.venv/bin/uvicorn \
  aletheia_memory.app:app --host 0.0.0.0 --port 8230
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
```

#### Manual systemctl commands

```bash
systemctl --user status aletheia aletheia-memory
systemctl --user restart aletheia
systemctl --user stop aletheia aletheia-memory
journalctl --user -u aletheia -f
journalctl --user -u aletheia-memory -f

# Required for services to survive logout (run once)
loginctl enable-linger
```

---

## Service Management

### macOS

```bash
aletheia start    # start services now
aletheia stop     # stop services
aletheia restart  # restart gateway

# Force restart via launchctl
launchctl kickstart -k gui/$(id -u)/com.aletheia.gateway
```

### Linux

```bash
aletheia start    # start services now
aletheia stop     # stop services
aletheia restart  # restart gateway

systemctl --user status aletheia aletheia-memory
systemctl --user restart aletheia
```

---

## Health Checks

```bash
aletheia doctor                            # config + connectivity check — no running gateway required
aletheia status                            # live metrics — requires running gateway

curl -s http://localhost:18789/health      # Gateway
curl -s http://localhost:6333/healthz      # Qdrant
curl -s http://localhost:7474              # Neo4j
curl -s http://localhost:8230/health       # Memory sidecar
```

---

## Troubleshooting

### Service won't start

1. Run `aletheia doctor` — this is always the first step. It checks config validity, connectivity, and required dependencies.
2. Check the log:
   - macOS: `tail -n 50 ~/.aletheia/logs/gateway.log`
   - Linux: `journalctl --user -u aletheia -n 50 --no-pager`

### Wrong node path in plist (macOS)

If Node.js was upgraded or moved, the installed plist has a stale `__NODE_BIN__` path.

```bash
aletheia disable && aletheia enable    # re-captures node path from current PATH

# Verify the resolved path
launchctl print gui/$(id -u)/com.aletheia.gateway | grep Program
```

### launchd not loading (macOS)

```bash
# Validate plist syntax
plutil ~/Library/LaunchAgents/com.aletheia.gateway.plist

# Verify log directory exists (launchd will not create parent directories)
ls ~/.aletheia/logs/

# If missing, aletheia enable creates it — re-run:
aletheia disable && aletheia enable
```

### Memory / graph services not working

```bash
# macOS
brew services list | grep -E "qdrant|neo4j"

# Linux
docker ps | grep -E "qdrant|neo4j"
# or
podman ps | grep -E "qdrant|neo4j"
```

### Reinstall

```bash
aletheia disable && aletheia enable
```

---

## Optional Integrations

### Signal (optional)

```bash
podman compose up -d    # Uses docker-compose.yml in repo root
```

Or native: `signal-cli -u +1XXXXXXXXXX daemon --http --receive-mode=on-start`

Configure via `channels.signal.accounts.default.httpPort` in gateway config.

### Langfuse (optional)

```bash
cd infrastructure/langfuse && podman compose up -d    # Port 3100
```

Set `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` in `~/.aletheia/env`.

### Prosoche (optional)

```bash
cd infrastructure/prosoche
cp config.yaml.example config.yaml && python3 prosoche.py
```
