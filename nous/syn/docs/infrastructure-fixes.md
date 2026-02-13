---
created: 2026-02-08
tags:
  - reference
aliases:
  - infrastructure fixes
---

# Aletheia Infrastructure Fixes

*Created: 2026-02-08 | Status: Active*
*Last updated by: Syn*

---

## Section 1: Safe for Syn (no restart/crash risk)

These can be done anytime without affecting the running gateway. No risk of downtime.

---

### 1.1 Fix cron job command names

**Problem:** Several cron entries reference commands that don't exist (`agent-digest`, `agent-status`). The actual tools are `nous-digest`, `nous-status`.

**Fix:**
```bash
crontab -e
# Replace all instances of:
#   agent-digest  →  nous-digest
#   agent-status  →  nous-status
#   bin/agent-status  →  nous-status
```

**Location:** `crontab -l` on worker-node (user: syn)

**Risk:** None. Cron jobs just silently fail currently.

---

### 1.2 Fix `nous-health` to read real session data

**Problem:** `nous-health` reports all agents as "stale/never" despite active sessions with 100K+ tokens. It's not reading from the correct session data.

**Location:** `/mnt/ssd/aletheia/shared/bin/nous-health`

**Fix:** The script needs to read from `/home/syn/.openclaw/agents/*/sessions/sessions.json` instead of whatever it currently checks. Each agent has a `sessions.json` with `updatedAt` timestamps and token counts.

**Session data structure:**
```
/home/syn/.openclaw/agents/
├── syn/sessions/sessions.json
├── syl/sessions/sessions.json
├── akron/sessions/sessions.json
├── chiron/sessions/sessions.json
├── eiron/sessions/sessions.json
├── demiurge/sessions/sessions.json
├── arbor/sessions/sessions.json
└── main/sessions/sessions.json    ← legacy, should only have DM session
```

Each `sessions.json` is a JSON object keyed by session key, with `sessionId`, `updatedAt` (unix ms), etc.

**Risk:** None — read-only monitoring tool.

---

### 1.3 Build `memory-consolidation` script

**Problem:** The 23:00 cron calls `memory-consolidation` which doesn't exist. Was either never built or lost in the naming migration.

**Location:** Should be at `/mnt/ssd/aletheia/shared/bin/memory-consolidation`

**Purpose:** Consolidate daily memory files across all nous workspaces. Promote significant findings from raw daily logs to curated MEMORY.md files.

**Related existing tools:**
- `memory-promote` — exists at `/mnt/ssd/aletheia/shared/bin/memory-promote`
- `consolidate-memory` — may exist (check shared/bin)
- `mine-memory-facts` — exists, extracts facts from memory files

**Fix:** Either create the script or update the cron to call the correct existing tool (`memory-promote`).

**Risk:** None.

---

### 1.4 Tooling audit cleanup (78 → ~57 scripts)

**Problem:** 78 scripts in shared/bin with redundancy, dead code, legacy naming.

**Analysis complete:** `/mnt/ssd/aletheia/nous/syn/memory/tooling-audit.md`

**Key actions:**
- Remove 5 duplicates (e.g., `patch-runtime` vs `patch-openclaw`)
- Archive 12 dead/broken scripts
- Merge 17 overlapping scripts in 8 groups
- Rename 8 scripts with legacy naming

**Risk:** Low — but verify each script isn't called by cron or other scripts before removing. Use `grep -r "scriptname" shared/bin/ crontab` to check dependencies.

---

### 1.5 Update `enforce-config` if nous list changes

**Problem:** If we add/remove a nous, the canonical registry is in the script.

**Location:** `/mnt/ssd/aletheia/shared/bin/enforce-config`

**Current registry:** main (Syn), akron, chiron, eiron, demiurge, syl, arbor

**Risk:** None — just edit the `NOUS` dict in the script.

---

### 1.6 Clean stale sessions (can be re-run anytime)

**Problem:** Old `agent:main:signal:group:...` sessions reappear when config reverts.

**Location:** `/home/syn/.openclaw/agents/main/sessions/sessions.json`

**Fix:**
```bash
python3 -c "
import json
f = '/home/syn/.openclaw/agents/main/sessions/sessions.json'
with open(f) as fh: s = json.load(fh)
stale = [k for k in s if 'signal:group:' in k]
for k in stale: del s[k]
with open(f, 'w') as fh: json.dump(s, fh, indent=2)
print(f'Cleaned {len(stale)} stale sessions')
"
```

Then reload: `config-reload`

**Risk:** None — only removes misrouted sessions. Each agent has their own correct sessions.

---

## Section 2: Requires Metis Claude Code

These involve gateway restarts, runtime code changes, service management, or credential re-auth that could interrupt Syn's session.

---

### 2.0 🔴 Session routing ignores bindings (FORK PATCH)

**Problem:** Gateway creates `agent:main:signal:group:...` sessions for ALL group messages, even when bindings explicitly map `agentId: "syl"` to that group. It appears to check for existing sessions under `main` before checking bindings, or defaults to `main` regardless.

**Evidence:** After cleaning stale sessions AND archiving transcript files, new messages to the Syl group immediately created a fresh `agent:main:signal:group:ieisvqz4k/...` session (new session ID, new transcript file at 08:23). The binding `agentId: "syl"` for that group ID is present and correct in config.

**Session data location:** `/home/syn/.openclaw/agents/main/sessions/sessions.json`

**Workaround applied:** `enforce-config` + archived stale transcripts to `/home/syn/.openclaw/agents/main/sessions/archived/`. But gateway re-creates them on new messages.

**Root cause location (likely):** Session routing logic in:
```
dist/gateway/session-utils.js    — lines 250-307 (session key construction)
dist/gateway/                    — look for binding resolution / agent selection logic
```

The routing likely: receives message → checks for existing session matching the group → finds/creates one under `main` → never checks bindings. Should be: receives message → checks bindings first → routes to correct agent → creates session under that agent.

**Testing:** After fix:
1. Message to Syl group → session key should be `agent:syl:signal:group:...`
2. Message to Akron group → session key should be `agent:akron:signal:group:...`
3. No `agent:main:signal:group:...` sessions should be created for bound groups

---

### 2.1 🔴 Gateway config overwrite bug (FORK PATCH)

**Problem:** The gateway writes its in-memory config to disk on startup, clobbering any changes made to `openclaw.json` while it was stopped or between restarts. Also happens during `config.patch` API calls.

**Current workaround:** `enforce-config` runs via cron every 15 minutes and re-patches the file + sends SIGUSR1. Fragile.

**Root cause location:** The config write-back logic is in the gateway startup code. Key files:
```
/mnt/ssd/aletheia/infrastructure/runtime/dist/gateway/config-reload.js
/mnt/ssd/aletheia/infrastructure/runtime/dist/config/schema.js
```

The gateway reads config, merges with defaults, then WRITES the merged result back to disk — overwriting any manual edits.

**Fix options:**
1. **Minimal:** Add a flag/env var that skips the config write-back on startup
2. **Better:** Make config.patch read from disk before merging (not from in-memory state)
3. **Best:** Treat disk as authoritative. Never write config unless explicitly asked.

**Gotchas:**
- All code is compiled JS in `dist/` — no TypeScript source in our fork
- Config schema is in `dist/config/schema.js` (~103 "agent" references)
- `config-reload.js` line 31: `{ prefix: "agents", kind: "none" }` — this is the reload trigger
- `session-utils.js` lines 250-307: session key prefix logic (`agent:`)

**Testing:** After patching, verify:
1. Edit openclaw.json while gateway is stopped → start gateway → config should be preserved
2. Run `config.patch` via API → disk file should reflect the patch
3. SIGUSR1 reload → should re-read from disk, not cache

---

### 2.2 🔴 Full terminology rename: `agent` → `nous` (FORK)

**Problem:** OpenClaw uses "agent" everywhere. Aletheia uses "nous". Current state: our tooling says nous, the runtime says agent. Cody's decision (Feb 7): Aletheia is canon, not a remix.

**Scope:**
```
249 files reference "agent" in dist/
~103 references in dist/config/schema.js alone
64 files in dist/gateway/
Session key prefix: "agent:" in dist/gateway/session-utils.js
```

**Approach:**
1. Build a rename map: `agent` → `nous`, `agents` → `nous`, `agentId` → `nousId`, `agentToAgent` → `nousToNous`
2. Create a re-runnable sed/python script that applies the rename to `dist/`
3. Test extensively — session routing, config parsing, CLI commands
4. Store the rename script in the repo so it can be re-applied after upstream pulls

**Key files to start with (highest impact):**
```
dist/config/schema.js              — config key definitions
dist/gateway/session-utils.js      — session key prefixes (lines 250-307)
dist/gateway/config-reload.js      — config section names
dist/cli/program/register.agent.js — CLI command registration
dist/agents/schema/                — agent schema definitions
```

**Gotchas:**
- "agent" appears in some contexts that AREN'T about our agents (e.g., user-agent headers, MCP agent references). The rename script needs to be targeted, not blind `sed`.
- Session state files on disk use `agent:` prefixed keys. Existing sessions need migration or will break.
- The CLI binary is still called `openclaw`. Renaming to `aletheia` is a separate step.
- `package.json` name is "openclaw" — affects npm resolution.

**Testing:** After rename, verify:
1. `aletheia nous list` shows all 7
2. Messages route correctly to each group
3. Session keys use `nous:` prefix
4. Config accepts `nous.list`, `nous.defaults`
5. Subagent spawning still works

---

### 2.3 🔴 Google Calendar re-auth

**Problem:** Both personal (`cody.kickertz@gmail.com`) and work (`ckickertz@summusglobal.com`) calendars return `invalid_grant` — OAuth token expired/revoked.

**Location:** Credentials are at `~/.config/gcal/` or wherever the gcal script stores them.

**Script:** `/mnt/ssd/aletheia/shared/bin/gcal`

**Fix:**
```bash
# On worker-node:
gcal auth
# Follow the OAuth flow in a browser
# May need to do separately for each calendar account
```

**Gotcha:** This requires browser-based OAuth consent. If running headless on worker-node, you may need to:
1. Run on Metis with display access, OR
2. Use `--no-browser` flag and manually open the URL, OR
3. Copy the token file from a machine with a browser

**Testing:** `gcal today -c cody.kickertz@gmail.com` should return events (or "no events").

---

### 2.4 🟡 Systemd service management

**Problem:** `systemctl --user` doesn't work on worker-node ("Failed to connect to bus: No medium found"). Gateway management is via raw kill signals.

**Location:** The OpenClaw systemd service was set up during onboarding but the user session bus isn't available.

**Likely cause:** The `syn` user doesn't have lingering enabled, or XDG_RUNTIME_DIR isn't set for the service context.

**Fix:**
```bash
# As root:
loginctl enable-linger syn

# Verify:
ls /run/user/$(id -u syn)/

# Then as syn:
export XDG_RUNTIME_DIR=/run/user/$(id -u)
systemctl --user status openclaw
```

**Gotcha:** If the service was started by root or a different mechanism, it may not be under systemd user control at all. Check:
```bash
ps -ef | grep openclaw  # Check PPID — is it systemd or something else?
```

Currently: PID 2789537 (openclaw) has PPID 1, meaning it's a direct child of init — likely started manually or by a system-level service, not user systemd.

**Alternative:** Create a proper system-level service at `/etc/systemd/system/openclaw.service` that runs as user `syn`. This is more reliable than user services for a headless server.

---

### 2.5 🟡 NAS SSH access

**Problem:** `ssh nas` returns "Permission denied" from worker-node.

**Expected:** nas = 192.168.0.120 (Synology 923+)

**Debug:**
```bash
ssh -vv nas 2>&1 | head -50     # verbose connection debug
ssh-copy-id syn@192.168.0.120   # if key not installed
cat ~/.ssh/config | grep -A5 nas # check config entry
```

**Gotcha:** Synology requires SSH key to be in the user's home directory with correct permissions (700 for .ssh, 600 for authorized_keys). DSM sometimes resets permissions on update.

---

### 2.6 🟡 Email (himalaya) setup

**Problem:** Email completely broken since Jan 30. Himalaya configured for Proton Bridge which isn't running.

**Location:** `~/.config/himalaya/config.toml` (on worker-node)

**Options:**
1. Set up Proton Bridge on worker-node (requires Proton subscription)
2. Switch to Gmail IMAP (app password) — simpler
3. Use Google API directly via existing OAuth

**Gotcha:** If using Gmail, need to enable "Less secure apps" or create an App Password. 2FA must be enabled first for app passwords.

**Testing:** `himalaya list -a personal` should show inbox.

---

### 2.7 🟢 Config compilation pipeline (future architecture)

**Problem:** We hand-edit `openclaw.json` and it gets clobbered. The real solution (post-fork-rename) is a compilation pipeline.

**Design:**
```
/mnt/ssd/aletheia/shared/config/topology.yaml   ← source of truth (our terms)
       ↓ compile-topology
/home/syn/.openclaw/openclaw.json                ← compiled output (runtime terms)
```

**This depends on:** 2.2 (terminology rename) being done first. No point compiling to a schema that's about to change.

**Build after:** The fork rename is complete and stable.

---

## Appendix: Key Paths Reference

| What | Path |
|------|------|
| OpenClaw config | `/home/syn/.openclaw/openclaw.json` |
| OpenClaw runtime | `/mnt/ssd/aletheia/infrastructure/runtime/` |
| Shared tools | `/mnt/ssd/aletheia/shared/bin/` |
| Nous workspaces | `/mnt/ssd/aletheia/nous/{syn,syl,chiron,eiron,demiurge,akron,arbor}/` |
| Session data | `/home/syn/.openclaw/agents/*/sessions/` |
| Theke (knowledge) | `/mnt/ssd/aletheia/theke/` |
| Config enforcer | `/mnt/ssd/aletheia/shared/bin/enforce-config` |
| Safe reload script | `/mnt/ssd/aletheia/shared/bin/config-reload` |
| Gateway logs | `journalctl --user -u aletheia` (broken — see 2.4) |
| Crontab | `crontab -l` (user: syn) |
| Gateway PID | `pgrep -a openclaw` |

## Appendix: Current Workarounds

| Issue | Workaround | Durability |
|-------|-----------|------------|
| Config overwrite | `enforce-config` cron every 15min | Holds until cron fails or service restarts faster than 15min |
| Stale sessions | Manual cleanup script | Must re-run whenever config reverts |
| Calendar auth | None — broken | Needs manual re-auth |
| Systemd | Raw `kill -USR1` signals | Works but no auto-restart on crash |

---

*This doc lives in theke for cross-machine sync. Metis Claude Code can read it directly.*
