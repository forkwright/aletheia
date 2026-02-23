# Codebase Concerns

**Analysis Date:** 2026-02-23

## Tech Debt

**Large Complex Modules:**
- Issue: Multiple files exceed 700+ lines, creating single points of complex logic
- Files:
  - `infrastructure/runtime/src/mneme/store.ts` (2395 lines)
  - `infrastructure/runtime/src/entry.ts` (940 lines)
  - `infrastructure/runtime/src/aletheia.ts` (882 lines)
  - `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (782 lines)
  - `infrastructure/runtime/src/semeion/listener.ts` (663 lines)
- Impact: Difficult to test in isolation, harder to review changes, increased likelihood of bugs in untested code paths
- Fix approach: Break into focused modules (< 300 lines each) with clear single responsibilities. Extract state machines, polling loops, and message routing into dedicated files

**Unused Auth Code:**
- Issue: Password-based auth scaffolded but not integrated (password mode in auth middleware exists but incomplete)
- Files: `infrastructure/runtime/src/auth/middleware.ts` (lines 89-100 handle password mode), `infrastructure/runtime/src/auth/passwords.ts` (complete but not wired)
- Impact: Dead code path that could mask bugs or cause unexpected behavior if accidentally activated
- Fix approach: Either complete password auth implementation with tests, or remove the scaffolded code. Add explicit `// TODO(unused)` markers as per CONTRIBUTING.md standards

**Minimal Error Context in Catches:**
- Issue: Several catch blocks silently ignore errors with only comments
- Files:
  - `infrastructure/runtime/src/aletheia.ts` (lines 138, 479, 688, 805, 810)
  - `infrastructure/runtime/src/organon/built-in/browser.ts` (line 55, 242)
  - `infrastructure/runtime/src/mneme/store.ts` (line 1841)
  - `infrastructure/runtime/src/auth/passwords.ts` (line 42)
- Impact: Silent failures make debugging difficult, failures in "best-effort" operations may indicate deeper problems
- Fix approach: Add structured logging to all catch blocks with error details, even for non-critical operations. Use `log.debug()` for expected failures, `log.warn()` for unexpected ones

## Known Limitations

**Distillation Lock Not Distributed:**
- Issue: Session distillation concurrency lock uses in-memory `Set<string>` (activeDistillations) at `infrastructure/runtime/src/distillation/pipeline.ts:19`
- Files: `infrastructure/runtime/src/distillation/pipeline.ts`
- Impact: Cannot support horizontal scaling — multiple runtime instances will attempt concurrent distillation of same session, causing conflicts
- Scaling path: Move activeDistillations to SessionStore as a distributed lock, or add per-session semaphore in database with TTL-based cleanup

**Browser Tool Not Resource-Limited Across Sessions:**
- Issue: Page count tracking is per-runtime global (`pageCount` variable at `infrastructure/runtime/src/organon/built-in/browser.ts:13`), not per-session
- Files: `infrastructure/runtime/src/organon/built-in/browser.ts`
- Impact: One aggressive session can exhaust MAX_PAGES and block other sessions from browsing. No fairness or queue
- Improvement path: Add per-session page budget or implement fair queue with session-level limits

**Tool Result Truncation Not Consistent:**
- Issue: Two separate limits exist for tool results — one in token counter and one in truncate stage
- Files:
  - `infrastructure/runtime/src/hermeneus/token-counter.ts` (TOOL_RESULT_CHAR_LIMITS and DEFAULT_RESULT_CHAR_LIMIT)
  - `infrastructure/runtime/src/nous/pipeline/stages/truncate.ts` (TOOL_RESULT_LIMITS duplicate definitions)
  - `infrastructure/runtime/src/distillation/chunked-summarize.ts` (MAX_TOOL_RESULT_CHARS = 8000)
- Impact: Tool results may be counted differently during execution vs. distillation, causing context estimation errors
- Fix approach: Centralize all tool result limits to single source of truth in `hermeneus/token-counter.ts`

**No Transactional Guarantees for Multi-Step Operations:**
- Issue: Session creation, thread binding, and distillation updates are separate database operations without transactions
- Files:
  - `infrastructure/runtime/src/mneme/store.ts` (createSession, resolveThread, linkSessionToThread are separate calls)
  - `infrastructure/runtime/src/distillation/pipeline.ts` (distillation updates session state in multiple steps)
- Impact: Process crash between operations leaves inconsistent state (orphaned threads, sessions in invalid state)
- Fix approach: Wrap multi-step operations in explicit transactions using better-sqlite3 transaction API

## Security Considerations

**Bearer Token Extraction From Query String:**
- Risk: Auth token accepted as both header (`Authorization: Bearer`) and query parameter (`?token=`)
- Files: `infrastructure/runtime/src/auth/middleware.ts` (lines 46-49)
- Current mitigation: Tokens in query params logged; HTTPS recommended in docs
- Recommendations:
  - Remove query string token support entirely — only accept Authorization header
  - Add warning log if query token is detected
  - Document that tokens in URLs are never acceptable for production

**Password Hash Parsing Without Validation:**
- Risk: Hash format parsing in `verifyPassword` assumes well-formed structure, could fail silently
- Files: `infrastructure/runtime/src/auth/passwords.ts` (lines 21-33)
- Current mitigation: Function returns `false` on any parsing error
- Recommendations:
  - Add strict validation of hash format before parsing (check part count, presence of required fields)
  - Log hash format errors as security events (potential tampering)
  - Consider using dedicated bcrypt library instead of rolling custom scrypt

**Encryption Key Derived Once at Startup:**
- Risk: `derivedKey` global holds sensitive material in memory for process lifetime
- Files: `infrastructure/runtime/src/koina/encryption.ts` (lines 21-26)
- Current mitigation: Key derived from passphrase + salt using PBKDF2, not stored on disk
- Recommendations:
  - Consider zero-ing key on shutdown
  - Add memory lock (mlock) to prevent key from being swapped to disk (platform-specific)
  - Document that encryption is only at-rest security; memory dumps expose plaintext

**MCP Token Comparison Timing Safe but Loaded From Disk:**
- Risk: MCP tokens loaded from JSON file with no integrity checking
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (lines 36-46)
- Current mitigation: Tokens compared using timingSafeEqual, preventing timing attacks
- Recommendations:
  - Add file integrity check (HMAC) to detect tampering with mcp-tokens.json
  - Validate token format and length before using them
  - Consider restricting file permissions (chmod 600)

## Performance Bottlenecks

**Multi-Stage Distillation Creates Combinatorial API Calls:**
- Problem: Distillation of large sessions calls LLM once per message chunk + once to merge summaries
- Files: `infrastructure/runtime/src/distillation/chunked-summarize.ts` (lines 110-145)
- Cause: Message chunking by token share, then independent summarization of each chunk
- Impact: 5+ message chunks = 6+ LLM calls for single distillation (expensive, high latency)
- Improvement path:
  - Pre-extract facts once, then chunk extracted data for parallel summarization
  - Use same LLM response for both extraction and initial summary
  - Implement sliding-window extraction to avoid re-processing messages

**Token Estimation Called Repeatedly for Same Content:**
- Problem: `estimateTokens()` called multiple times for same message during a single turn
- Files: Used throughout `infrastructure/runtime/src/nous/pipeline/stages/execute.ts`
- Cause: No caching layer, algorithm is complex (multiple regex passes)
- Impact: Non-trivial CPU during tool execution loops
- Improvement path: Cache token estimates by content hash during turn execution

**Browser Page Cleanup Uses Timeout Plus Finalizer:**
- Problem: Page cleanup relies on both timer and finally block, potential for double-cleanup race
- Files: `infrastructure/runtime/src/organon/built-in/browser.ts` (lines 59-66)
- Cause: Timer-based cleanup (PAGE_TIMEOUT=30s) + explicit cleanup on return/error
- Impact: If function completes just before timeout, both cleanup paths fire (harmless due to `cleaned` flag, but inefficient)
- Improvement path: Use AbortController with timeout instead of setTimeout

## Fragile Areas

**Loop Detection State Management:**
- Files: `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (loopDetector used throughout execute function)
- Why fragile: Loop detection tracks tool calls by name/input across turns, but state persists in generator function scope
- If generator suspended/resumed unexpectedly or message history replayed, loop counts could become inconsistent
- Safe modification: Add explicit loop detector reset at turn boundaries, log state on suspends
- Test coverage: Loop detection test exists (`nous/pipeline/stages/` tests), but edge cases around generator suspension not covered

**Message Queue Mid-Turn Draining:**
- Files: `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (lines 462-484)
- Why fragile: Queued messages injected after tool execution, requiring careful message ordering
- Risk: Multiple tool calls within same turn + queue drain could result in out-of-order messages or duplicates
- Safe modification: Add explicit ordering markers to messages, validate sequence on append
- Test coverage: Basic queue test exists, but concurrent send + tool execution race condition not tested

**Session Type Lifecycle (primary/background/ephemeral):**
- Files: `infrastructure/runtime/src/mneme/store.ts` (Session.sessionType field), `infrastructure/runtime/src/nous/ephemeral.ts` (ephemeral limit enforcement)
- Why fragile: Ephemeral sessions have hard MAX_CONCURRENT limit (3) but no queue — requests above limit reject immediately
- Sessions created with `sessionType` but no validation of transition rules
- Safe modification: Add explicit state machine for session type transitions, enforce valid transitions
- Test coverage: Ephemeral limit test exists, but graceful degradation when limit exceeded not tested

**Context Management API Ordering:**
- Files: `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (lines 75-80, buildContextManagement function)
- Why fragile: Comment explicitly states "Thinking clearing must come first in edits array (API requirement)"
- Hard-coded ordering assumption that could break if Anthropic API changes
- Safe modification: Add defensive validation that thinking clear is at index 0, fail fast with clear error
- Test coverage: No test for context management API ordering

## Potential Race Conditions

**Active Turns Decrement Race:**
- Risk: `activeTurns` counter in NousManager decremented in finally block, but could be read by concurrent code
- Files: `infrastructure/runtime/src/nous/manager.ts` (activeTurns tracked but incremented/decremented without locks)
- Mitigation: JavaScript single-threaded, but if future code uses Worker threads this becomes critical
- Recommendation: Document that activeTurns is not thread-safe, add mutex wrapper if multithreading introduced

**Concurrent Browser Page Allocation:**
- Risk: `pageCount` check and increment are separate operations (`if (pageCount >= MAX_PAGES)` then `pageCount++`)
- Files: `infrastructure/runtime/src/organon/built-in/browser.ts` (lines 42, 48)
- Mitigation: Single-threaded execution, but between async operations another request could allocate page
- Recommendation: Use atomic compare-and-swap or mutex for page count

**Distillation Active Set Not Cleaned on Process Exit:**
- Risk: If distillation function throws, `activeDistillations.delete()` may not execute
- Files: `infrastructure/runtime/src/distillation/pipeline.ts` (lines 71-80 add to set, line 119 removes)
- Mitigation: Delete wrapped in finally block, so should be safe
- Recommendation: Verify finally always executes in async generator edge cases

## Test Coverage Gaps

**Distillation Under Memory Pressure:**
- What's not tested: Behavior when distillation runs while session is actively receiving new messages
- Files: `infrastructure/runtime/src/distillation/pipeline.test.ts` (tests prevent concurrent distillation, but not interaction with live turns)
- Risk: Message written during distillation could be lost if transaction boundaries wrong
- Priority: Medium — affects multi-user scenarios

**Tool Timeout Edge Cases:**
- What's not tested: Tool timeout followed immediately by successful result from same tool
- Files: `infrastructure/runtime/src/organon/timeout.ts` (timeout enforcement), `infrastructure/runtime/src/nous/pipeline/stages/execute.ts` (error handling)
- Risk: Timeout marker in message history + successful result = confusing state
- Priority: Medium — affects flaky tools and network delays

**Browser Page Cleanup Under Stress:**
- What's not tested: Multiple concurrent page requests when already at MAX_PAGES=3
- Files: `infrastructure/runtime/src/organon/built-in/browser.ts` (withPage function)
- Risk: Requests rejected without queue, users get immediate "max pages reached" error
- Priority: Low — expected behavior, but could add queue + backoff

**Auth Token Rotation / Refresh:**
- What's not tested: Multiple Aletheia instances with same auth token, one instance compromised
- Files: `infrastructure/runtime/src/auth/middleware.ts`, `infrastructure/runtime/src/auth/tokens.ts`
- Risk: No token revocation mechanism, no session invalidation on compromise
- Priority: Low for single-instance, High for multi-instance deployments

**MCP Message Size Validation:**
- What's not tested: Message field exceeds MAX_MESSAGE_BYTES (100KB) validation
- Files: `infrastructure/runtime/src/pylon/mcp.ts` (MAX_MESSAGE_BYTES defined but validation not visible)
- Risk: Oversized messages could cause crashes or exploits
- Priority: Medium — affects external tool integration

## Dependencies at Risk

**better-sqlite3 Version Lock:**
- Risk: Database library is performance-critical and actively maintained; pinned version may have bugs
- Impact: Switching versions requires validation of all transaction behavior
- Migration plan: Regular updates recommended, with regression test suite run

**playwright-core Browser Automation:**
- Risk: Requires local Chromium binary; CHROMIUM_PATH env var not validated
- Impact: Missing or wrong Chromium path fails silently until browser tool called
- Migration plan: Add startup validation of Chromium availability, provide helpful error messages

**Anthropic SDK Context Management API:**
- Risk: Context management `clear_thinking_20251015` and `clear_tool_uses_20250919` versioned with dates
- Impact: API changes may break distillation without warning
- Migration plan: Monitor SDK changelog, wrap API calls with version negotiation

## Environmental Assumptions

**Encryption Passphrase Must Survive Process Restart:**
- Issue: Passphrase passed to initEncryption() at startup, then discarded
- Impact: Cannot re-initialize encryption after restart without storing passphrase somewhere
- Assumption: Passphrase sourced from environment variable `ALETHEIA_ENCRYPTION_KEY` or user input during init
- Verify: Check paths.ts and loader.ts for how passphrase is obtained

**Signal-CLI Daemon External Dependency:**
- Issue: Requires signal-cli service running separately, restarted with max 5 attempts
- Files: `infrastructure/runtime/src/semeion/daemon.ts` (MAX_RESTART_ATTEMPTS=5)
- Impact: signal-cli crashes = messaging disabled, but no fallback
- Current: Logs error and gives up after 5 restarts
- Verify: Deployment docs for signal-cli setup

## Scaling Limits

**Current Capacity:**
- Max concurrent ephemeral agents: 3 (hard limit)
- Max concurrent browser pages: 3 (global, not per-agent)
- Max pending session sends: 5 per session
- Max concurrent turns per listener: 6 (hard limit)
- Distillation concurrency: 1 per session (no horizontal scaling)

**Identified Bottlenecks:**
- Single-instance limitation: All state tracking uses in-memory collections
- Database: better-sqlite3 is local-only (no network replication)
- Browser pool: Single Chromium process across all sessions

**Scaling Path:**
- Sessions: Move to shared database with distributed locks (Postgres, MySQL, etc.)
- Ephemeral agents: Implement queue + fairness scheduler
- Browser: Consider dedicated browser pool service
- Distillation: Add distributed lock mechanism (Redis or DB-backed)

---

*Concerns audit: 2026-02-23*
