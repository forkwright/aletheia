# Spec: Security Hardening — Sandbox, Encryption, Audit, PII

**Status:** Phase 1 done (PII detection). Phases 2-4 remaining.
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-4, F-6, F-7, F-8; OwnPilot + OpenClaw reference implementations

---

## Problem

Aletheia runs with full system access. Agents execute arbitrary shell commands, read/write anywhere on disk, and send messages to external services. The security model is trust-based: agents are configured not to do harmful things, but nothing prevents them mechanically.

Four gaps, ordered by practical impact:

1. **No PII awareness.** Memories extracted from conversations may contain phone numbers, emails, addresses, SSNs. These are stored in plaintext vectors and could surface in any agent's recall. Signal outbound messages aren't screened.

2. **No execution sandbox.** `exec` runs commands as the agent's OS user with full filesystem and network access. A malicious tool result or prompt injection could escalate.

3. **No memory encryption.** Qdrant vectors and Neo4j properties store conversation extractions in plaintext. Database file theft exposes everything.

4. **No audit integrity.** The audit log table records events but has no tamper detection. An attacker (or a bug) could modify or delete audit entries undetected.

---

## Design

### Phase 1: PII Detection & Redaction (F-8)

The most practically valuable security feature. Runs on three surfaces:

**Detection** — `koina/pii.ts` module with pattern-based detectors:
- Phone numbers (US/international formats)
- Email addresses
- SSN / tax IDs
- Street addresses (heuristic)
- Credit card numbers (Luhn validation)
- API keys / tokens (entropy + prefix detection)
- Names in sensitive contexts (medical, financial)

Each detector returns matches with confidence scores (0-1) and span positions.

**Redaction modes:**
- `mask` — replace with `[REDACTED:type]` (e.g., `[REDACTED:phone]`)
- `hash` — replace with deterministic hash (preserves referential equality)
- `warn` — log but don't modify (audit mode)

**Integration points:**
1. **Memory storage** — run before Mem0 `add`. Redact or flag PII in memories.
2. **Signal outbound** — run before `message` tool sends. Block or redact PII in messages to groups.
3. **LLM context** — optionally run on system prompt assembly to redact PII from recalled memories before sending to API.

**Configuration:**
```yaml
pii:
  mode: mask          # mask | hash | warn
  surfaces:
    memory: true      # scan before memory storage
    outbound: true    # scan before Signal sends
    context: false    # scan before LLM calls (performance cost)
  allowlist:          # known-safe patterns (e.g., Cody's own email)
    - cody.kickertz@*
    - ckickertz@summusglobal.com
```

### Phase 2: Docker Sandbox (F-4)

Execution isolation for the `exec` tool.

**Scope** — only `exec` tool calls. Other tools (read, write, edit) operate on the local filesystem and need direct access.

**Implementation:**
1. **Pattern pre-screen** — before execution, check command against deny patterns:
   - Network access (`curl`, `wget`, `nc`, `ssh` to non-allowlisted hosts)
   - Privilege escalation (`sudo`, `su`, `chmod +s`)
   - Destructive operations (`rm -rf /`, `mkfs`, `dd`)
   - Known exploit patterns
   
2. **Docker container** — for commands that pass pre-screen, optionally execute in a container:
   - Read-only bind mount of workspace
   - No network (unless explicitly allowed)
   - Non-root user
   - Memory/CPU limits
   - Config-hash based image selection (reproducible environments)
   - Env sanitization (no host secrets leaked)

3. **Opt-in** — sandbox is configurable per-agent and per-command. Some commands need host access (git, systemctl). Sandbox is the default for sub-agent spawns; persistent nous get direct access by default.

```yaml
sandbox:
  enabled: true
  mode: docker          # docker | pattern-only
  image: aletheia-sandbox:latest
  allowNetwork: false
  mountWorkspace: readonly
  denyPatterns:
    - "rm -rf /"
    - "sudo *"
    - "chmod +s *"
  bypassFor: [main, demiurge]  # persistent nous bypass sandbox
```

### Phase 3: Tamper-Evident Audit Trail (F-7)

Add integrity verification to the audit log.

**Hash chain** — each audit entry includes:
- `checksum` — SHA-256 of the entry's content
- `previous_checksum` — SHA-256 of the previous entry

```sql
ALTER TABLE audit_log ADD COLUMN checksum TEXT;
ALTER TABLE audit_log ADD COLUMN previous_checksum TEXT;
```

**Verification** — `aletheia audit verify` walks the chain and reports:
- Chain length and time span
- First/last entry
- Any broken links (missing or modified entries)
- Any gaps in sequence IDs

**Write path** — audit log writes compute the chain:
```typescript
function appendAuditEntry(entry: AuditEntry): void {
  const lastChecksum = store.getLastAuditChecksum();
  const content = JSON.stringify({ ...entry, previous_checksum: lastChecksum });
  const checksum = sha256(content);
  store.insertAudit({ ...entry, checksum, previous_checksum: lastChecksum });
}
```

### Phase 4: Encrypted Memory (F-6)

AES-256-GCM encryption for memory storage.

**Scope:**
- Qdrant vector metadata (the text content, not the vectors themselves — vectors are already lossy representations)
- Neo4j node/edge properties
- SQLite memory tables

**Key management:**
- Master key derived from passphrase via PBKDF2 (100K iterations)
- Data encryption key (DEK) encrypted by master key
- DEK rotation without re-encrypting all data (envelope encryption)
- Key stored in `ALETHEIA_ENCRYPTION_KEY` env var or keyfile

**Performance consideration:** Encryption/decryption adds latency to every memory read/write. For Qdrant, this means vector search returns encrypted metadata that must be decrypted before use. Benchmark before deploying — if latency exceeds 50ms overhead, consider caching decrypted results.

**Threat model clarity:** This protects against database file theft (someone copies `sessions.db` or Qdrant's data directory). It does NOT protect against a compromised runtime (which has the key in memory). Document this explicitly so we don't create false confidence.

---

## Implementation Order

| Phase | What | Effort | Features |
|-------|------|--------|----------|
| **1** | PII detection & redaction | Medium | F-8 |
| **2** | Docker sandbox + pattern pre-screen | Medium | F-4 |
| **3** | Tamper-evident audit trail | Small | F-7 |
| **4** | Encrypted memory | Medium | F-6 |

---

## Evaluation Items

| Question | What to determine |
|----------|-------------------|
| Docker on worker-node | Is Docker available? Performance cost of container spin-up per exec call? |
| Encryption performance | Overhead on search latency? Acceptable threshold? |
| PII detector accuracy | False positive rate on technical content (UUIDs, hex strings, IP addresses)? |
| Key management UX | Passphrase on startup vs. keyfile vs. env var? Recovery story? |

---

## Success Criteria

- Zero PII in stored memories (or all flagged/redacted)
- Signal outbound messages screened before send
- Audit log chain verifiable end-to-end with `aletheia audit verify`
- Sub-agent exec calls sandboxed by default
- Memory encryption transparent to agents (no code changes needed)
- Security features configurable — can be disabled for development
