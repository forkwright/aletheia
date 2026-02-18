# Spec: Data Privacy & Encryption at Rest

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  
**Depends on:** [spec-auth-and-security.md](spec-auth-and-security.md) (TLS, session auth)

---

## Problem

Aletheia stores deeply personal data — conversation transcripts, health information, family details, relationship context, financial data, work artifacts, personal preferences, and extracted memories — all in plaintext across multiple storage backends. Anyone with filesystem access (including other processes on the machine, backup systems, or a compromised service) can read everything.

This is not hypothetical. The system currently stores:

### Data Inventory

| Store | Location | Contents | Size | Encryption |
|-------|----------|----------|------|------------|
| **Session DB** | `~/.aletheia/sessions.db` | All conversations, tool outputs, usage data, cross-agent messages, contact requests, blackboard entries | 6.7MB (4500+ messages) | None |
| **Qdrant vectors** | `data/qdrant/` | Extracted memories as embeddings + plaintext payloads | 12MB | None |
| **Neo4j graph** | `data/neo4j/` | Knowledge graph — entity relationships, episodes, community structure | 526MB | None |
| **facts.jsonl** | `shared/memory/facts.jsonl` | Structured facts about the user (136KB) | 136KB | None |
| **Agent workspaces** | `nous/*/` | MEMORY.md, daily logs, USER.md, research notes | ~1MB total | None |
| **Config** | `~/.aletheia/aletheia.json` | Auth token (plaintext), LLM API keys (env vars), Neo4j password | 8KB | None |
| **Signal data** | `signal-cli/` | Signal encryption keys, message history, contact database | varies | Signal protocol (E2E) |
| **Docker volumes** | various | Neo4j/Qdrant persistent data | varies | None |

### What's Actually Sensitive

**Tier 1 — Critical (breach = real harm):**
- Conversation content (health, financial, family, personal)
- Extracted memories in Qdrant (concentrated personal facts)
- Knowledge graph relationships (who knows whom, health conditions, etc.)
- Signal encryption keys
- API keys and auth tokens in config

**Tier 2 — Sensitive (breach = privacy violation):**
- facts.jsonl (structured personal data)
- MEMORY.md files (operational context with personal details)
- USER.md (personal profile)
- Daily memory logs
- Cross-agent messages (may contain forwarded personal content)

**Tier 3 — Operational (breach = system compromise):**
- Session metadata (who talked when, which agent)
- Usage/token tracking
- Routing cache
- Blackboard entries

### Current Vulnerabilities

1. **SQLite DB is world-readable** — `644` permissions on `sessions.db`. Any process on the machine can read all conversations.
2. **Config is group-readable** — `664` on `aletheia.json` containing the auth token in plaintext.
3. **No disk encryption on data volumes** — Qdrant/Neo4j data dirs are plaintext on `/mnt/ssd/`.
4. **Neo4j password in docker-compose.yml** — `NEO4J_AUTH: neo4j/aletheia-memory` hardcoded in version-controlled file.
5. **Qdrant has no authentication** — Bound to localhost only, but any local process can query it.
6. **Memory sidecar has no authentication** — HTTP on port 8230 with no auth. Any local process can add/search/delete memories.
7. **Backup exposure** — No encrypted backup strategy. If sessions.db or data dirs are backed up (rsync, NAS sync, etc.), plaintext copies proliferate.
8. **Log exposure** — journalctl logs may contain message content in error cases. Agent workspace logs (extraction.log, etc.) may contain personal data.
9. **Export API serves full conversation history** — `/api/export/sessions/:id` returns complete message content as NDJSON. Combined with token-in-URL vulnerability from auth spec, this is a one-click data exfiltration.
10. **Memory sidecar proxied without additional auth** — Gateway proxies `/api/memory/*` to sidecar. Token auth is the only gate — but that's the same static token that's leaked via SSE URL params.
11. **Conversation content sent to LLM providers** — Anthropic, Voyage (embeddings). This is inherent to the architecture but should be documented and controllable.

---

## Design: Encryption at Rest

### Philosophy

Defense in depth. Multiple independent layers, each protecting against different threat models:

- **Filesystem permissions** → protects against other local users/processes
- **Application-level encryption** → protects against filesystem access (backup leak, disk theft)
- **Database-level encryption** → protects the primary data store
- **Service authentication** → protects against local process compromise

No single layer is sufficient. All are necessary.

### Phase 1: Permissions Hardening (Zero dependencies)

**Immediate, no code changes required.**

```bash
# Config file — owner-only read
chmod 600 ~/.aletheia/aletheia.json

# Session database — owner-only read/write
chmod 600 ~/.aletheia/sessions.db

# Agent workspaces — owner + group (agents share a group)
chmod 750 /mnt/ssd/aletheia/nous/*/
chmod 640 /mnt/ssd/aletheia/nous/*/MEMORY.md
chmod 640 /mnt/ssd/aletheia/nous/*/USER.md

# Shared memory — owner + group
chmod 640 /mnt/ssd/aletheia/shared/memory/shared-memory/facts.jsonl

# Neo4j/Qdrant data — restrict to container user
chmod 700 /mnt/ssd/aletheia/data/qdrant/
chmod 700 /mnt/ssd/aletheia/data/neo4j/
```

**Also:** Remove Neo4j password from `docker-compose.yml`. Move to `.env` file (gitignored) or Docker secrets.

### Phase 2: SQLite Encryption (SQLCipher)

**Why:** The session database is the single richest target. 4500+ messages, every conversation, every tool output, cross-agent messages, contact requests. Encrypting it protects against filesystem access, backup leaks, and disk theft.

**Implementation: SQLCipher via `better-sqlite3-multiple-ciphers`**

Drop-in replacement for `better-sqlite3` that supports SQLCipher-compatible encryption.

```typescript
// mneme/store.ts — modified constructor
import Database from "better-sqlite3-multiple-ciphers";

export class SessionStore {
  private db: Database.Database;

  constructor(dbPath: string, encryptionKey?: string) {
    this.db = new Database(dbPath);
    
    if (encryptionKey) {
      // SQLCipher: AES-256-CBC with HMAC-SHA512
      this.db.pragma(`key = '${encryptionKey}'`);
      this.db.pragma("cipher_compatibility = 4"); // SQLCipher 4 format
    }
    
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("synchronous = NORMAL");
    this.db.pragma("foreign_keys = ON");
    this.init();
  }
}
```

**Key management:**

```yaml
# aletheia.json (new field)
{
  "session": {
    "encryption": {
      "enabled": true,
      "keySource": "env"  // "env" | "file" | "keyring"
    }
  }
}
```

- **`env`** — Key from `ALETHEIA_DB_KEY` env var (simple, works with systemd `EnvironmentFile=`)
- **`file`** — Key from `~/.aletheia/db.key` (600 permissions, not in git)
- **`keyring`** — Linux kernel keyring (most secure, requires `keyctl`)

**Migration path:**
1. Export existing DB: `sqlite3 sessions.db .dump > sessions.sql`
2. Create encrypted DB: open with key, import dump
3. CLI command: `aletheia migrate-db --encrypt`

**Performance:** SQLCipher adds ~5-15% overhead on reads/writes. For a system doing maybe 100 DB operations per conversation turn, this is negligible.

### Phase 3: Qdrant Authentication & Encryption

**Problem:** Qdrant runs on localhost:6333 with zero auth. Any process can query, add, or delete memories.

**Fix 1 — API key authentication:**

```yaml
# docker-compose.yml (Qdrant)
services:
  qdrant:
    environment:
      QDRANT__SERVICE__API_KEY: "${QDRANT_API_KEY}"
```

Then pass the key through the memory sidecar and mem0 config:

```python
# config.py
QDRANT_API_KEY = os.environ.get("QDRANT_API_KEY", "")

MEM0_CONFIG = {
    "vector_store": {
        "provider": "qdrant",
        "config": {
            "api_key": QDRANT_API_KEY,
            # ... existing config
        }
    }
}
```

**Fix 2 — Qdrant storage encryption:**

Qdrant doesn't support encryption at rest natively. Two options:
- **LUKS/dm-crypt volume** for the Qdrant data directory (OS-level, transparent)
- **eCryptfs** mount on `data/qdrant/` (lighter weight, user-space)

Recommended: **LUKS** if the SSD isn't already encrypted. **eCryptfs** if you want per-directory granularity.

### Phase 4: Neo4j Security

**Current state:** Password hardcoded in docker-compose.yml, no TLS between gateway/sidecar and Neo4j.

**Fixes:**

1. **Move credentials to `.env`** (gitignored):
   ```env
   NEO4J_AUTH=neo4j/${STRONG_RANDOM_PASSWORD}
   ```

2. **Enable Neo4j native auth + TLS:**
   ```yaml
   environment:
     NEO4J_dbms_ssl_policy_bolt_enabled: "true"
     NEO4J_dbms_ssl_policy_bolt_base__directory: /ssl
     NEO4J_dbms_connector_bolt_tls__level: REQUIRED
   ```

3. **Role separation:** Create a read-only Neo4j user for the sidecar's search queries. Admin operations (graph export, normalization) use the admin user.

### Phase 5: Memory Sidecar Authentication

**Problem:** The sidecar on port 8230 has zero auth. Any local process can:
- Add fake memories (`POST /add`)
- Search all memories (`POST /search`)
- Delete any memory (`DELETE /memories/:id`)
- Export the entire graph (`GET /graph/export`)
- Trigger consolidation/normalization

**Fix: API key middleware**

```python
# routes.py — add auth middleware
from fastapi import Depends, Security
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials

security = HTTPBearer(auto_error=False)
SIDECAR_API_KEY = os.environ.get("ALETHEIA_MEMORY_KEY", "")

async def verify_key(credentials: HTTPAuthorizationCredentials = Security(security)):
    if not SIDECAR_API_KEY:
        return  # No key configured = open (backward compat)
    if not credentials or credentials.credentials != SIDECAR_API_KEY:
        raise HTTPException(status_code=401, detail="Invalid API key")

# Apply to all routes
router = APIRouter(dependencies=[Depends(verify_key)])
```

The gateway passes the key when proxying:
```typescript
const memoryHeaders: Record<string, string> = {
  "Content-Type": "application/json",
  ...(process.env["ALETHEIA_MEMORY_KEY"] 
    ? { "Authorization": `Bearer ${process.env["ALETHEIA_MEMORY_KEY"]}` } 
    : {}),
};
```

### Phase 6: Conversation Content Protection

**The big question: Should message content be encrypted in SQLite even beyond SQLCipher?**

SQLCipher encrypts the entire database file. But if the application is running, the DB is decrypted in memory. Anyone who can access the running process (or the API) can read messages.

**Column-level encryption** would encrypt `messages.content` with a separate key, so even with DB access, content is opaque:

```typescript
import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

const ALGO = "aes-256-gcm";

function encryptContent(plaintext: string, key: Buffer): { ciphertext: string; iv: string; tag: string } {
  const iv = randomBytes(16);
  const cipher = createCipheriv(ALGO, key, iv);
  const encrypted = Buffer.concat([cipher.update(plaintext, "utf8"), cipher.final()]);
  return {
    ciphertext: encrypted.toString("base64"),
    iv: iv.toString("base64"),
    tag: cipher.getAuthTag().toString("base64"),
  };
}

function decryptContent(encrypted: { ciphertext: string; iv: string; tag: string }, key: Buffer): string {
  const decipher = createDecipheriv(ALGO, Buffer.from(encrypted.iv, "base64"), key);
  decipher.setAuthTag(Buffer.from(encrypted.tag, "base64"));
  return Buffer.concat([
    decipher.update(Buffer.from(encrypted.ciphertext, "base64")),
    decipher.final(),
  ]).toString("utf8");
}
```

**Trade-offs:**
- 🟢 Messages are encrypted even when DB file is decrypted
- 🟢 Protects against API-level data exfiltration (export endpoints return ciphertext)
- 🔴 Cannot search/query message content in SQL
- 🔴 Breaks `getHistory()` — need to decrypt on read
- 🔴 ~10-20% overhead per message read/write
- 🔴 Distillation needs to decrypt, summarize, re-encrypt

**Recommendation:** Implement SQLCipher (Phase 2) first. Column-level encryption is overkill for a single-user self-hosted system unless you're concerned about the export API being used for exfiltration — which is better solved by the RBAC system in the auth spec (restrict export to admin role).

### Phase 7: Workspace File Encryption

Agent workspace files (MEMORY.md, USER.md, daily logs) contain personal data in plaintext markdown.

**Options:**

1. **eCryptfs on `nous/` directory** — transparent encryption, files appear plaintext when mounted
2. **LUKS volume for entire `/mnt/ssd/aletheia`** — encrypts everything, simplest
3. **Application-level encryption** per sensitive file — complex, breaks agent tools (grep, read)

**Recommendation:** **LUKS/full-disk encryption on the SSD**. This is the right level of abstraction — it protects all data stores (SQLite, Qdrant, Neo4j, workspace files, config) with a single unlock at boot, and doesn't break any application functionality.

If the SSD is already on a LUKS volume, this is already done. If not, this is a one-time migration.

---

## Design: Data Minimization

Encryption protects data that exists. Minimization reduces what exists.

### Conversation Retention Policy

Currently, all messages are kept forever (even "distilled" ones — they're just flagged, not deleted).

**Proposed policy:**

```yaml
# aletheia.json
{
  "session": {
    "retention": {
      "activeSessionMaxAge": "90d",      // Auto-archive after 90 days inactive
      "archivedRetention": "365d",        // Delete archived sessions after 1 year
      "distilledMessageRetention": "30d", // Delete distilled (summarized) messages after 30 days
      "toolResultRetention": "7d",        // Tool results are bulky and ephemeral
      "usageDataRetention": "365d"        // Keep usage/cost data for a year
    }
  }
}
```

**Implementation:** Cron job runs daily, enforces retention:

```typescript
// daemon/retention.ts
export function enforceRetention(store: SessionStore, policy: RetentionPolicy): void {
  // 1. Archive stale active sessions
  store.archiveInactiveSessions(policy.activeSessionMaxAge);
  
  // 2. Delete old archived sessions entirely  
  store.purgeArchivedSessions(policy.archivedRetention);
  
  // 3. Delete distilled messages (the summary replaces them)
  store.purgeDistilledMessages(policy.distilledMessageRetention);
  
  // 4. Delete old tool results (keep tool name, drop content)
  store.truncateToolResults(policy.toolResultRetention);
  
  // 5. Aggregate and purge granular usage data
  store.aggregateUsageData(policy.usageDataRetention);
}
```

### Memory Retention

Qdrant memories should also have a lifecycle:

- **Confidence decay** — memories accessed less over time get lower scores
- **Periodic consolidation** — merge near-duplicates (already implemented in sidecar)
- **Explicit purge** — user command to delete all memories matching a pattern
- **Right to forget** — `aletheia forget "topic"` removes all memories, graph nodes, and facts related to a topic

### Log Sanitization

**Problem:** Error logs may contain message content. Example from journalctl:
```
Stream error: Invalid state: Controller is already closed
```

That's fine. But if an error occurs during message processing, the log could contain:
```
Failed to process message: "Hey Syn, my doctor said my A1C is 8.2..."
```

**Fix:** Log sanitizer that strips message content from error context:

```typescript
function sanitizeForLog(text: string): string {
  if (text.length > 200) return text.slice(0, 50) + "...[redacted]..." + text.slice(-20);
  return text;
}
```

Apply to all `log.error()` calls that include user content.

---

## Design: LLM Provider Data Flow

### What Goes to External Services

| Provider | What | Why | Controllable? |
|----------|------|-----|---------------|
| Anthropic (Claude) | Full conversation context, system prompts, tool results | Core LLM inference | No (required for function) |
| Anthropic (Haiku) | User queries (for memory search rewriting) | Enhanced search | Yes — disable `rewrite: false` |
| Voyage AI | Conversation chunks (for embedding) | Memory storage | Yes — use local embedder |
| Anthropic (Haiku) | Conversation excerpts (for fact extraction) | Memory extraction | Yes — disable memory plugin |

### Data Flow Controls

```yaml
# aletheia.json — new privacy section
{
  "privacy": {
    "memoryExtraction": true,       // Send conversations to LLM for fact extraction
    "embeddingProvider": "local",    // "local" (fastembed) or "voyage"
    "searchRewriting": true,         // LLM-powered search query rewriting
    "telemetry": false,              // No telemetry (already the case)
    "conversationSharing": "none"    // "none" | future: "anonymized-research"
  }
}
```

When `embeddingProvider: "local"`, the sidecar uses fastembed (GTE-large, runs locally) instead of Voyage API. This keeps all memory processing on-device.

### Provider Data Handling

Document in README/privacy policy:
- Anthropic: [Usage policy](https://www.anthropic.com/policies/usage-policy) — API data not used for training by default
- Voyage AI: Check their data retention policy
- All providers: Data in transit protected by TLS (their endpoints)
- No conversation data is stored by providers beyond their standard API processing

---

## Design: Secure Export & Data Portability

### Current Export Problem

The export API (`/api/export/sessions/:id`) returns full conversation content as plaintext NDJSON. Combined with the auth spec's finding that the token leaks via URL params, this is effectively unauthenticated access to all conversations.

### Encrypted Export

```typescript
// New endpoint: /api/export/encrypted
app.post("/api/export/encrypted", async (c) => {
  const { password, nousId, since, until } = await c.req.json();
  
  // Derive key from password using scrypt
  const salt = randomBytes(32);
  const key = scryptSync(password, salt, 32);
  
  // Collect all session data
  const sessions = store.listSessionsFiltered({ nousId, since, until });
  const exportData = sessions.map(s => ({
    session: s,
    messages: store.getHistory(s.id),
    usage: store.getUsageForSession(s.id),
  }));
  
  // Encrypt with AES-256-GCM
  const iv = randomBytes(16);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  const encrypted = Buffer.concat([
    cipher.update(JSON.stringify(exportData), "utf8"),
    cipher.final(),
  ]);
  
  return new Response(
    JSON.stringify({
      version: 1,
      algorithm: "aes-256-gcm",
      kdf: "scrypt",
      salt: salt.toString("base64"),
      iv: iv.toString("base64"),
      tag: cipher.getAuthTag().toString("base64"),
      data: encrypted.toString("base64"),
    }),
    { headers: { "Content-Type": "application/json" } }
  );
});
```

**CLI decryption:**
```bash
aletheia export decrypt --file export.json --password "..."
```

### Data Portability

The user should be able to:
1. **Export everything** — conversations, memories, knowledge graph, config
2. **In an encrypted archive** — password-protected, portable
3. **With a clear schema** — documented JSON format for each data type
4. **And delete the original** — "right to erasure" command that removes all data from all stores

```bash
# Full encrypted export
aletheia export --all --encrypt --output ~/aletheia-backup.enc

# Selective export
aletheia export --memories --encrypt --output ~/memories.enc
aletheia export --conversations --since 2026-01-01 --output ~/chats.enc

# Nuclear option — delete everything
aletheia purge --confirm "DELETE ALL DATA"
```

---

## Implementation Priority

| Phase | What | Effort | Impact | Dependencies |
|-------|------|--------|--------|--------------|
| **1** | File permissions hardening | 30 min | High | None |
| **2** | Move secrets to `.env` / gitignore | 1 hour | High | None |
| **3** | SQLCipher for sessions.db | 1 day | Critical | npm package swap |
| **4** | Qdrant API key auth | 2 hours | High | Docker restart |
| **5** | Memory sidecar API key | 2 hours | High | Code change |
| **6** | Conversation retention policy | 1 day | Medium | Cron infrastructure |
| **7** | Log sanitization | 2 hours | Medium | Code review |
| **8** | LUKS full-disk encryption | 2 hours | Critical | Downtime, backup first |
| **9** | Encrypted export | 1 day | Medium | Auth spec Phase 2 (RBAC) |
| **10** | Column-level encryption | 2 days | Low | SQLCipher first |
| **11** | Data portability CLI | 1 day | Medium | Export infrastructure |

**Phase 1-2 can be done today with zero code changes.**  
**Phase 3-5 are the highest-impact code changes.**  
**Phase 8 (LUKS) is the single most effective protection** — it encrypts everything at once.

---

## Threat Model Summary

| Threat | Current Protection | After This Spec |
|--------|-------------------|-----------------|
| Remote attacker (no auth) | Static bearer token | Session auth + TLS (auth spec) |
| Local process reads DB | None (644 perms) | 600 perms + SQLCipher |
| Local process queries Qdrant | None (no auth) | API key auth |
| Local process queries sidecar | None (no auth) | API key auth |
| Disk theft / lost laptop | None | LUKS encryption |
| Backup leak (rsync, NAS) | None | SQLCipher + LUKS |
| API token exfiltration | Token in URL | Session auth (auth spec) |
| Export API data leak | Bearer token only | RBAC + encrypted export |
| Log file exposure | Unredacted | Log sanitization |
| LLM provider data access | Inherent | Local embeddings option |
| Memory poisoning | No auth on sidecar | API key + input validation |
| Neo4j credential leak | Hardcoded in compose | .env file, TLS |

---

## What This Doesn't Cover

- **E2E encryption between user and agent** — messages are decrypted at the gateway for LLM processing. This is inherent to the architecture.
- **Homomorphic encryption on LLM inference** — not practical with current technology.
- **Multi-user data isolation** — single-user system. If multi-user is added later, per-user encryption keys and tenant isolation are needed.
- **Key rotation** — covered conceptually (SQLCipher supports `PRAGMA rekey`) but full rotation ceremony deferred.
- **HSM/TPM key storage** — overkill for self-hosted, but the `keyring` option for DB key approaches this.
- **Secure deletion/shredding** — SQLCipher's `PRAGMA cipher_memory_security` helps, but SSD wear-leveling makes true secure deletion unreliable. LUKS + TRIM is the practical answer.
