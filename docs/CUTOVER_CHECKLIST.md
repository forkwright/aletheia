# Cutover Checklist: TS Runtime to Rust Binary on NixOS

> Production cutover plan for replacing the Node.js/TypeScript runtime with the
> Rust binary on the production server. Covers the current Ubuntu deployment and
> the path to NixOS.
>
> **Date:** 2026-03-15
> **Status:** Ready (GO recommendation from P114 validation, 2026-03-08)

---

## Table of contents

1. [Server audit](#1-server-audit)
2. [Instance directory compatibility](#2-instance-directory-compatibility)
3. [NixOS flake status](#3-nixos-flake-status)
4. [Build verification](#4-build-verification)
5. [Cutover procedure](#5-cutover-procedure)
6. [Rollback plan](#6-rollback-plan)
7. [Post-cutover cleanup](#7-post-cutover-cleanup)
8. [Observations](#8-observations)

---

## 1. Server audit

### Machine

| Property | Value |
|----------|-------|
| Hostname | worker-node |
| OS | Ubuntu 24.04 LTS |
| Access | `ssh syn@100.87.6.45` (Tailscale) or `ssh server-lan` (LAN) |
| User | `syn` (passwordless sudo) |
| Dev clone | `/mnt/ssd/dev/aletheia/` |
| Live repo | `/home/syn/aletheia/` (always on `main`) |
| Instance root | `/home/syn/aletheia/instance/` |

### Running services

| Service | Type | Port | Status after cutover |
|---------|------|------|---------------------|
| `aletheia.service` | systemd (system) | 18789 | **Replace** (TS → Rust) |
| `aletheia-memory.service` | systemd | 8230 | **Kill** (Mem0 sidecar, replaced by mneme) |
| `aletheia-prosoche.service` | systemd | — | **Kill** (embedded in Rust binary) |
| qdrant | Docker | 6333 | **Kill** (replaced by mneme embedded HNSW) |
| neo4j | Docker | 7474, 7687 | **Kill** (replaced by mneme embedded CozoDB) |
| langfuse | Docker | 3100 | **Keep** (observability) |
| signal-cli | process | 8080 | **Keep** (optional, not day-1) |

### Current systemd unit (`/etc/systemd/system/aletheia.service`)

```ini
[Service]
User=syn
Group=syn
WorkingDirectory=/home/syn/aletheia/instance/nous/syn
EnvironmentFile=/home/syn/aletheia/instance/config/aletheia.env
Environment=PATH=/home/syn/aletheia/instance/shared/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/usr/local/bin/aletheia gateway run
Restart=on-failure
RestartSec=10
MemoryMax=8G
MemorySwapMax=2G
```

Key differences for the Rust binary:
- `ExecStart` drops `gateway run` (Rust binary serves by default)
- `WorkingDirectory` changes to `/home/syn/aletheia` (repo root, not agent dir)
- `MemoryMax` can drop to `2G` (Rust uses ~50-100 MB vs Node's ~240 MB)
- Add `--instance-root /home/syn/aletheia/instance` flag

### Current binary

`/usr/local/bin/aletheia` is a symlink to `infrastructure/runtime/aletheia.mjs` (Node.js).
The TS runtime was deleted in PR #601 — the symlink target may already be broken.

### Configuration files (production)

| File | Format | Consumer |
|------|--------|----------|
| `instance/config/aletheia.json` | JSON | TS runtime (legacy) |
| `instance/config/aletheia.env` | env | systemd EnvironmentFile |
| `instance/config/credentials/anthropic.json` | JSON | Both (OAuth with auto-refresh) |

### Docker containers

```
qdrant       — qdrant/qdrant        — 6333, 6334
neo4j        — neo4j:5              — 7474, 7687
langfuse     — langfuse/langfuse    — 3100
```

### Pre-cutover server verification commands

Run these on the server before starting the cutover:

```bash
# Service status
sudo systemctl status aletheia aletheia-memory aletheia-prosoche
sudo docker ps --format "table {{.Names}}\t{{.Ports}}\t{{.Status}}"
sudo ss -tlnp | grep -E "18789|8230|6333|7474|7687|3100|8080"

# Disk usage
df -h /home
sudo du -sh /home/syn/aletheia/ /var/lib/docker/

# Config files
ls -la /home/syn/aletheia/instance/config/
cat /home/syn/aletheia/instance/config/aletheia.env

# Credential validity
python3 -c "
import json, sys
c = json.load(open('/home/syn/aletheia/instance/config/credentials/anthropic.json'))
print(f'type={c.get(\"type\", \"static\")} expires={c.get(\"expiresAt\", \"n/a\")}')
"

# Ownership
stat -c "%U:%G %a %n" /home/syn/aletheia/instance/ /home/syn/aletheia/instance/config/ /home/syn/aletheia/instance/data/
```

---

## 2. Instance directory compatibility

### Config format mismatch

| Item | Production (TS) | Rust binary |
|------|----------------|-------------|
| Config file | `aletheia.json` | `aletheia.toml` (primary) or `aletheia.yaml` (deprecated) |
| Config loader | Custom JSON | figment cascade (defaults → TOML → env vars) |
| Auth model | Session-based (username/password) | JWT bearer token |
| Agent config key | `agentId` + `match.peer` | flat `channel`/`source`/`nousId` |

**Action required:** Create `instance/config/aletheia.toml` from scratch. The Rust binary
does NOT read `aletheia.json`. Both files can coexist — the JSON stays for rollback.

### Config cascade

Resolution order (later wins):
1. Compiled defaults (`AletheiaConfig::default()`)
2. TOML file at `{instance}/config/aletheia.toml`
3. Environment variables prefixed `ALETHEIA_` (double underscore for nesting)

### Instance root discovery

Resolution order (first match wins):
1. `--instance-root` CLI flag
2. `ALETHEIA_ROOT` environment variable
3. `./instance` relative to working directory

### Required directory structure

The Rust binary validates these at startup:

```
instance/
├── config/              # MUST exist
│   ├── aletheia.toml    # Main config (create from example)
│   └── credentials/     # API keys (already exists)
│       └── anthropic.json
├── data/                # MUST exist, MUST be writable
│   ├── sessions.db      # Auto-created by Rust binary
│   ├── planning.db      # Auto-created
│   └── knowledge.fjall/ # Auto-created
├── nous/                # Warning if missing (not fatal)
│   ├── syn/SOUL.md      # REQUIRED per agent
│   ├── demiurge/SOUL.md
│   ├── syl/SOUL.md
│   └── akron/SOUL.md
├── logs/                # Auto-created
├── shared/              # Optional
└── theke/               # Optional
```

### Credential compatibility

**Compatible.** The credential file format is identical:

```json
{
  "token": "sk-ant-oat-...",
  "refreshToken": "your-refresh-token",
  "expiresAt": 0
}
```

Credential resolution chain:
1. `instance/config/credentials/anthropic.json` (file, with OAuth refresh)
2. `ANTHROPIC_AUTH_TOKEN` env var
3. `ANTHROPIC_API_KEY` env var
4. Claude Code credentials at `~/.claude/.credentials.json`

The existing `anthropic.json` on the server works with both runtimes.

### Session DB path

TS runtime: sessions stored in its own format.
Rust binary: `{instance_root}/data/sessions.db` (SQLite, auto-created).

**No migration needed.** The Rust binary creates a fresh session store. Historical
TS sessions are not carried over (acceptable — sessions are ephemeral).

### Agent workspace paths

The TS runtime used `instance/nous/{agent}/` directories.
The Rust binary uses the same paths, configured in `aletheia.toml`:

```toml
[[agents.list]]
id = "syn"
default = true
workspace = "nous/syn"  # Relative to instance root
```

**SOUL.md is required per agent.** All other bootstrap files (IDENTITY.md, GOALS.md,
MEMORY.md, etc.) are optional and gracefully skipped if absent.

### Binding schema translation

TS format → Rust format mapping for Signal bindings:

```
TS:   {"agentId": "syl", "match": {"channel": "signal", "peer": {"kind": "dm", "id": "UUID"}}}
Rust: { channel = "signal", source = "UUID", nousId = "syl" }
```

The bindings must be manually translated when creating `aletheia.toml`.

### Environment variables

| TS runtime | Rust binary | Notes |
|-----------|-------------|-------|
| (read from aletheia.env) | `ALETHEIA_ROOT` | Instance root |
| — | `ALETHEIA_GATEWAY__PORT` | Override port |
| — | `ALETHEIA_GATEWAY__BIND` | Override bind address |
| `ANTHROPIC_API_KEY` | `ANTHROPIC_API_KEY` | Same (fallback to credential file) |
| — | `ALETHEIA_JWT_SECRET` | JWT signing key (new) |
| — | `RUST_LOG` | Logging level |

### Mismatches summary

| Mismatch | Severity | Resolution |
|----------|----------|------------|
| Config format (JSON → TOML) | High | Create `aletheia.toml` from template |
| Auth model (session → JWT) | Medium | Set `gateway.auth.mode` and JWT secret |
| Binding schema | Medium | Translate `match.peer` to flat `source` |
| WorkingDirectory in systemd | Low | Update to repo root |
| ExecStart arguments | Low | Remove `gateway run` |
| MemoryMax | Low | Reduce from 8G to 2G |

---

## 3. NixOS flake status

### Current state

**No `flake.nix` exists in the repo.** The aletheia repo has no Nix files at all.
NixOS is the target deployment platform but the flake has never been written.

### What's planned

The [nix-integration.md](../planning/nix-integration.md) plan defines a phased approach:

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Minimal `flake.nix` (crane build + dev shell) | Not started |
| Phase 2 | NixOS module (`services.aletheia.enable`) | Not started |
| Phase 3 | Infrastructure repo + NixOS install on Zephyrus | Not started |
| Phase 4 | NVIDIA/CUDA support | Not started |

### Blocking issues for `nix build`

1. **No flake.nix to build.** Must be written from scratch.
2. **Dependency inventory** (from nix-integration.md):
   - `libsqlite3-sys` (bundled) — needs `cc` in nativeBuildInputs (easy)
   - `onig_sys` (Oniguruma regex) — needs `cc` (easy)
   - `ring` (TLS crypto) — needs `cc` + asm (well-tested in nixpkgs)
   - `spm_precompiled` (sentencepiece) — needs `cc` (easy)
3. **TLS status:** After rustls-only migration (completed), openssl-sys is eliminated.
   Build only needs: `cc`, no `pkg-config`, no `openssl.dev`, no `cmake`.
4. **Embedding model:** BGE-small-en-v1.5 (~130 MB) downloaded at first run via hf-hub.
   Nix plan calls for pre-fetching as fixed-output derivation.

### NixOS module design (planned)

```nix
services.aletheia = {
  enable = true;
  settings = {
    gateway.port = 18789;
    gateway.bind = "lan";
    agents.list = [{ id = "syn"; default = true; }];
    embedding.provider = "candle";
  };
};
```

Generates `aletheia.toml` from Nix expressions. Instance data at `/var/lib/aletheia/`.

### Recommendation

**NixOS is NOT a blocker for cutover.** The current server is Ubuntu 24.04. The cutover
should proceed on Ubuntu with `cargo build` + systemd. NixOS migration is a separate,
later effort targeting the Zephyrus hardware.

---

## 4. Build verification

### Prior validation (P114, 2026-03-08)

| Check | Result |
|-------|--------|
| `cargo build --release` | Clean, 49 MB binary, 5m10s |
| `cargo test --workspace` | 1,462 tests, 0 failures |
| `cargo clippy --workspace` | Zero warnings |
| `cargo fmt --check` | Clean |
| Health endpoint | Responds (degraded without credentials — expected) |
| Bootstrap | SOUL.md required, all others graceful skip |
| Tool parity | 32 tools registered, all TS-era tools present |
| Signal integration | Graceful degradation if unconfigured |
| Distillation | Working, state survives |

### System dependencies for building

| Dependency | Required | Notes |
|-----------|----------|-------|
| Rust 1.85+ | Yes | Edition 2024 |
| `cc` (gcc/clang) | Yes | For ring, libsqlite3-sys, onig_sys |
| `pkg-config` | No | Eliminated after rustls-only migration |
| `openssl-dev` | No | Eliminated after rustls-only migration |
| `cmake` | No | Eliminated after aws-lc-sys removal |

### Build on server

```bash
# On worker-node
cd /mnt/ssd/dev/aletheia
git checkout main && git pull origin main
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
# Binary at: target/release/aletheia
```

### Binary verification

```bash
# Check it's a real ELF binary
file target/release/aletheia
# Expected: ELF 64-bit LSB pie executable, x86-64

# Check it starts
ALETHEIA_ROOT=/home/syn/aletheia/instance \
  target/release/aletheia health
# Expected: healthy or degraded

# Validate config without starting
target/release/aletheia check-config \
  --instance-root /home/syn/aletheia/instance

# Show help
target/release/aletheia --help
```

### CLI subcommands available

```
aletheia [OPTIONS] [COMMAND]

Commands:
  init           Interactive instance setup
  health         Check server health
  check-config   Validate config without starting
  status         Show system status
  credential     Manage credentials
  tui            Terminal dashboard
  backup         Database backups
  maintenance    Background tasks
  tls            TLS certificate management
```

---

## 5. Cutover procedure

### Pre-cutover backup

```bash
# 1. Backup the entire instance directory
ssh syn@100.87.6.45 'tar czf /home/syn/aletheia-backup-$(date +%Y%m%d-%H%M%S).tar.gz \
  -C /home/syn/aletheia instance/'

# 2. Backup the current systemd unit
ssh syn@100.87.6.45 'sudo cp /etc/systemd/system/aletheia.service \
  /home/syn/aletheia-service-backup.service'

# 3. Backup the current binary (or symlink)
ssh syn@100.87.6.45 'cp -P /usr/local/bin/aletheia /home/syn/aletheia-binary-backup'

# 4. Verify backups exist
ssh syn@100.87.6.45 'ls -lh /home/syn/aletheia-backup-*.tar.gz /home/syn/aletheia-service-backup.service'
```

### Step 1: Build release binary

```bash
ssh syn@100.87.6.45 'cd /mnt/ssd/dev/aletheia && \
  git checkout main && git pull origin main && \
  export PATH="$HOME/.cargo/bin:$PATH" && \
  cargo build --release 2>&1 | tail -5'

# Verify
ssh syn@100.87.6.45 'file /mnt/ssd/dev/aletheia/target/release/aletheia'
```

### Step 2: Create Rust config (`aletheia.toml`)

Create `instance/config/aletheia.toml` on the server. Template below — adapt
from the existing `aletheia.json` values:

```toml
# Aletheia Rust Runtime Configuration
# Cascade: compiled defaults -> this file -> ALETHEIA_* env vars

[gateway]
port = 18789
bind = "lan"

[gateway.auth]
mode = "token"

[gateway.csrf]
enabled = false

[agents.defaults.model]
primary = "claude-opus-4-6"
fallbacks = ["claude-sonnet-4-6"]

[agents.defaults]
contextTokens = 200000
maxOutputTokens = 16384
userTimezone = "America/Chicago"
timeoutSeconds = 300
maxToolIterations = 50

[[agents.list]]
id = "syn"
name = "Syn"
default = true
workspace = "nous/syn"
domains = ["orchestration", "code", "systems"]

[[agents.list]]
id = "akron"
name = "Akron"
workspace = "nous/akron"
domains = ["vehicle", "radio", "preparedness"]

[[agents.list]]
id = "demiurge"
name = "Demiurge"
workspace = "nous/demiurge"
domains = ["leather", "craft"]

[[agents.list]]
id = "syl"
name = "Syl"
workspace = "nous/syl"
domains = ["family", "home"]

[channels.signal]
enabled = false  # Enable after basic cutover is validated

# Bindings — enable after Signal is verified
# [[bindings]]
# channel = "signal"
# source = "48d8b030-eb68-4440-a749-dc35c67876e7"
# nousId = "syl"
#
# [[bindings]]
# channel = "signal"
# source = "*"
# nousId = "syn"

[embedding]
provider = "candle"
dimension = 384

[maintenance.traceRotation]
maxAgeDays = 14
compress = true

[maintenance.dbMonitoring]
warnThresholdMb = 100
alertThresholdMb = 500
```

```bash
# Copy config to server
scp /tmp/aletheia.toml syn@100.87.6.45:/home/syn/aletheia/instance/config/aletheia.toml

# Verify it parses
ssh syn@100.87.6.45 '/mnt/ssd/dev/aletheia/target/release/aletheia check-config \
  --instance-root /home/syn/aletheia/instance'
```

### Step 3: Set up JWT secret

```bash
# Generate a secure JWT signing key
ssh syn@100.87.6.45 'openssl rand -base64 32 >> /home/syn/aletheia/instance/config/env'
# Edit the env file to set: ALETHEIA_JWT_SECRET=<generated-key>

# Or set auth mode to "none" initially for testing:
# [gateway.auth]
# mode = "none"
```

### Step 4: Parallel smoke test on temp port

Start the Rust binary on a different port to validate before touching the live service:

```bash
ssh syn@100.87.6.45

# Run on temp port 19000
ALETHEIA_GATEWAY__PORT=19000 \
ALETHEIA_ROOT=/home/syn/aletheia/instance \
RUST_LOG=aletheia=info \
/mnt/ssd/dev/aletheia/target/release/aletheia \
  --instance-root /home/syn/aletheia/instance \
  --bind 127.0.0.1 \
  2>&1 | tee /tmp/aletheia-rust-smoke.log &
RUST_PID=$!
sleep 5

# Validate
curl -s http://127.0.0.1:19000/api/health | python3 -m json.tool
curl -s http://127.0.0.1:19000/api/v1/nous | python3 -m json.tool

# Check for errors
grep -iE 'error|panic|fatal' /tmp/aletheia-rust-smoke.log

# Credential check
/mnt/ssd/dev/aletheia/target/release/aletheia credential status \
  --instance-root /home/syn/aletheia/instance

# Kill temp instance
kill $RUST_PID && wait $RUST_PID 2>/dev/null
exit
```

**All checks must pass before proceeding.**

### Step 5: Install binary

```bash
ssh -t syn@100.87.6.45 'sudo rm /usr/local/bin/aletheia && \
  sudo cp /mnt/ssd/dev/aletheia/target/release/aletheia /usr/local/bin/aletheia && \
  sudo chmod 755 /usr/local/bin/aletheia'

# Verify it's the Rust ELF binary
ssh syn@100.87.6.45 'file /usr/local/bin/aletheia'
```

### Step 6: Update systemd unit

```bash
cat > /tmp/aletheia.service << 'UNIT'
[Unit]
Description=Aletheia — Distributed Cognition System
After=network.target docker.service

[Service]
Type=simple
User=syn
Group=syn
WorkingDirectory=/home/syn/aletheia
EnvironmentFile=/home/syn/aletheia/instance/config/aletheia.env
Environment=ALETHEIA_ROOT=/home/syn/aletheia/instance
Environment=RUST_LOG=aletheia=info
ExecStart=/usr/local/bin/aletheia --instance-root /home/syn/aletheia/instance
Restart=on-failure
RestartSec=10
MemoryMax=2G

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/home/syn/aletheia/instance /mnt/ssd/aletheia
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
UNIT

scp /tmp/aletheia.service syn@100.87.6.45:/tmp/aletheia.service
ssh -t syn@100.87.6.45 'sudo cp /tmp/aletheia.service /etc/systemd/system/aletheia.service && \
  sudo systemctl daemon-reload'
```

### Step 7: Service stop sequence (order matters)

```bash
ssh -t syn@100.87.6.45

# Record pre-cutover state
date -Iseconds > /tmp/cutover-timestamp.txt
echo "Pre-cutover snapshot" >> /tmp/cutover-timestamp.txt
ps aux | grep aletheia >> /tmp/cutover-timestamp.txt

# 1. Stop the TS runtime FIRST
sudo systemctl stop aletheia
sleep 2
curl -sf http://127.0.0.1:18789/api/health && echo "ERROR: still running!" || echo "TS stopped OK"

# 2. Stop sidecars (order doesn't matter among these)
sudo systemctl stop aletheia-memory 2>/dev/null
sudo systemctl stop aletheia-prosoche 2>/dev/null

# 3. Start the Rust runtime
sudo systemctl start aletheia
sleep 3

# 4. Immediate verification
sudo systemctl status aletheia --no-pager
curl -s http://127.0.0.1:18789/api/health | python3 -m json.tool
```

### Step 8: Post-cutover verification

```bash
# Health endpoint
curl -s http://127.0.0.1:18789/api/health | python3 -m json.tool
# Expected: {"status": "healthy", ...}

# Agents loaded
curl -s http://127.0.0.1:18789/api/v1/nous | python3 -m json.tool
# Expected: all 4 agents (syn, akron, demiurge, syl)

# Metrics endpoint
curl -s http://127.0.0.1:18789/metrics | head -20

# Error check
sudo journalctl -u aletheia --since "5 minutes ago" --no-pager | grep -iE 'error|panic|warn'

# Memory usage (should be ~50-100 MB vs Node's ~240 MB)
ps aux | grep '/usr/local/bin/aletheia' | grep -v grep | awk '{print "RSS:", $6/1024, "MB"}'

# Confirm sidecars are stopped
sudo systemctl status aletheia-memory --no-pager 2>&1 | head -3
sudo systemctl status aletheia-prosoche --no-pager 2>&1 | head -3

# Confirm langfuse still running
curl -sf http://127.0.0.1:3100 > /dev/null && echo "Langfuse OK" || echo "Langfuse DOWN"
```

---

## 6. Rollback plan

If anything goes wrong after the swap, revert in under 2 minutes:

```bash
ssh -t syn@100.87.6.45

# 1. Stop Rust binary
sudo systemctl stop aletheia

# 2. Restore original binary
sudo rm /usr/local/bin/aletheia
sudo cp /home/syn/aletheia-binary-backup /usr/local/bin/aletheia
# Or if backup was a symlink:
# sudo ln -s /home/syn/aletheia/infrastructure/runtime/aletheia.mjs /usr/local/bin/aletheia

# 3. Restore original systemd unit
sudo cp /home/syn/aletheia-service-backup.service /etc/systemd/system/aletheia.service
sudo systemctl daemon-reload

# 4. Restart sidecars if needed
sudo systemctl start aletheia-memory 2>/dev/null
sudo systemctl start aletheia-prosoche 2>/dev/null

# 5. Start TS runtime
sudo systemctl start aletheia
sleep 3

# 6. Verify
curl -s http://127.0.0.1:18789/api/health | python3 -m json.tool
```

**Config safety:** The TS runtime reads `aletheia.json`, the Rust binary reads
`aletheia.toml`. Both files coexist — rollback is config-safe.

**Data safety:** The Rust binary creates its own `sessions.db`. The TS session
data (if any) is in its own format. Rolling back does not corrupt either.

---

## 7. Post-cutover cleanup

Perform only after **48 hours of stable operation**.

### Disable old sidecars permanently

```bash
# Disable sidecars so they don't restart on reboot
sudo systemctl disable aletheia-memory
sudo systemctl disable aletheia-prosoche

# Remove unit files (optional, after confirming stability)
# sudo rm /etc/systemd/system/aletheia-memory.service
# sudo rm /etc/systemd/system/aletheia-prosoche.service
# sudo systemctl daemon-reload
```

### Remove Docker containers (qdrant, neo4j)

```bash
# Stop and remove containers
sudo docker stop qdrant neo4j
sudo docker rm qdrant neo4j

# Remove images (optional, saves disk)
sudo docker rmi qdrant/qdrant neo4j:5

# Verify langfuse still running
sudo docker ps
```

### Clean up old data

```bash
# Remove qdrant data (verify path first)
# sudo rm -rf /var/lib/qdrant/  # or wherever qdrant stored data

# Remove neo4j data (verify path first)
# sudo rm -rf /var/lib/neo4j/   # or wherever neo4j stored data
```

### Record the cutover

```bash
cat >> /home/syn/aletheia/instance/CUTOVER.md << EOF
# Cutover Record
- Date: $(date -Iseconds)
- From: Node.js runtime (infrastructure/runtime/aletheia.mjs)
- To: Rust binary (/usr/local/bin/aletheia)
- Config: instance/config/aletheia.toml (taxis/figment)
- Previous config retained: instance/config/aletheia.json (rollback reference)
- Services removed: aletheia-memory, aletheia-prosoche, qdrant, neo4j
- Services retained: langfuse, signal-cli
EOF
```

### Enable Signal (day 2+)

After basic cutover is verified stable, enable Signal integration:

1. Edit `instance/config/aletheia.toml`:
   ```toml
   [channels.signal]
   enabled = true

   [channels.signal.accounts.primary]
   name = "Aletheia"
   enabled = true
   account = "+15124288605"
   httpHost = "localhost"
   httpPort = 8080
   autoStart = false
   dmPolicy = "open"
   groupPolicy = "allowlist"
   requireMention = true
   ```

2. Uncomment the `[[bindings]]` entries.

3. Restart: `sudo systemctl restart aletheia`

4. Test: Send a Signal message and verify response.

---

## 8. Observations

### Security

- The systemd unit currently uses `MemoryMax=8G` with `MemorySwapMax=2G`. The Rust
  binary uses ~50-100 MB — `MemoryMax=2G` is generous.
- The new unit adds `ProtectSystem=strict`, `NoNewPrivileges=yes`, `PrivateTmp=yes` —
  hardening the Rust service beyond what the TS runtime had.
- `ProtectHome=read-only` with `ReadWritePaths` scoping limits blast radius.
- JWT auth replaces session-based auth. A proper `ALETHEIA_JWT_SECRET` must be set
  before exposing the API on LAN.

### Operational risks

- **TS runtime may already be broken.** PR #601 deleted the TS infrastructure code.
  If the server has pulled `main` since then, the symlink at `/usr/local/bin/aletheia`
  points to a deleted file. Verify before assuming rollback to TS is viable.
- **First embedding model download.** With `embedding.provider = "candle"`, the binary
  downloads BGE-small-en-v1.5 (~130 MB) on first startup via hf-hub. This requires
  outbound HTTPS to huggingface.co. Ensure network access or pre-download the model.
- **Session history not migrated.** The Rust binary creates fresh session databases.
  TS-era conversation history is not carried over. This is acceptable for cutover
  but should be communicated to users.

### NixOS path

The current cutover targets Ubuntu. The path to NixOS:

1. Complete this cutover on Ubuntu (validate the Rust binary in production)
2. Write `flake.nix` (Phase 1 of nix-integration.md) — 1-2 days
3. Write NixOS module (Phase 2) — 1 day
4. Create `aletheia-infra` repo with Zephyrus host config (Phase 3) — 2-3 days
5. Install NixOS on Zephyrus and migrate instance data
6. Optional: CUDA support (Phase 4)

The Ubuntu cutover and NixOS migration are independent steps. The NixOS flake
does not need to exist for the cutover to proceed.

### Stale services

The `aletheia-memory` (Mem0 sidecar) and `aletheia-prosoche` services are
remnants of the TS architecture. Both capabilities are now embedded in the Rust
binary. They should be stopped during cutover and disabled after stability
confirmation.

### Docker footprint

After removing qdrant and neo4j containers, only langfuse remains in Docker.
Consider whether langfuse should also move to a native service or remain
containerized. Not a cutover concern — langfuse is independent.
