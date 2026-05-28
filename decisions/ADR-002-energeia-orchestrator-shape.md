<!-- Operator-review-pending: agent-drafted under DIRECTIVE v20; T0 metis review required before status moves Proposed → Accepted -->
# ADR-001: energeia orchestrator shape

## Status

Proposed

## Context

Energeia is the aletheia dispatch crate: it plans work, executes prompt groups, monitors session outcomes, accounts for budget, and reports quality results. The most important boundary is not a vendor API client; it is the `DispatchEngine` trait that lets orchestration logic treat every provider-backed run as a spawned session with events, completion, and abort semantics.

The current codebase has two execution shapes behind that boundary. One shape wraps a generic `hermeneus::provider::LlmProvider`, allowing the provider layer to adapt concrete transports such as Claude Code, Codex, or Kimi. The other shape, despite names like `HttpEngine` and `AgentSdkEngine`, is explicit that the production transport is currently a local Claude CLI subprocess rather than a direct native HTTP/SSE or Agent SDK client. That naming history matters because it captures the architectural pressure: callers should not care whether a provider is reached through a local OAuth'd CLI, a provider SDK, or a direct HTTP token, but the operationally stable path today is the local CLI.

The CLI pattern is visible in energeia itself:

```text
crates/energeia/src/engine.rs:27
pub trait DispatchEngine: Send + Sync {
```

```text
crates/energeia/src/http/client.rs:3
//! Named `HttpEngine` because the trait targets the Anthropic Agent SDK HTTP/SSE
//! API. The current implementation uses the Claude CLI subprocess as a transport
//! (matching phronesis's approach) because the Agent SDK HTTP endpoints are not
//! yet publicly documented. The [`DispatchEngine`] trait boundary insulates
//! callers from this implementation detail.
```

```text
crates/energeia/src/agent_sdk.rs:71
/// Experimental Claude CLI dispatch engine.
///
/// WHY: Provides CLI subprocess integration with `OAuth` token injection,
/// permissions, and MCP configuration fields while the native SDK path remains
/// unwired.
```

The wider provider layer uses the same local process model for concrete providers. Claude Code, Codex, and Kimi are invoked through provider-specific binaries, with stdout/stderr captured and authentication state delegated to the installed CLI:

```text
crates/hermeneus/src/cc/process.rs:134
let mut cmd = Command::new(cc_binary);
```

```text
crates/hermeneus/src/codex/process.rs:78
let mut cmd = Command::new(codex_binary);
```

```text
crates/hermeneus/src/kimi/process.rs:75
fn build_kimi_command(kimi_binary: &Path, cwd: &Path, model: &str) -> Command {
```

This leaves a decision to record. Energeia could become the place where provider API tokens, token refresh, SDK quirks, HTTP retry policy, and provider streaming protocols are all implemented directly. Or it can remain an orchestrator that starts local provider sessions, lets each provider CLI use its own OAuth store and policy model, and receives normalized session events/results through internal traits.

## Decision

**Energeia keeps the dispatch model centered on spawned local provider sessions, with provider-specific CLI or provider adapters behind `DispatchEngine`, rather than making energeia the owner of direct provider API-token clients.**

The preferred production path is the OAuth'd local-CLI pattern when a provider CLI exists and already owns login, token refresh, permission prompts, and workspace-level behavior. Energeia may wrap that path directly, as with the Claude subprocess engines, or indirectly through `HermeneusEngine` and a `LlmProvider`. In both cases, orchestration code sees the same session-oriented interface: `spawn_session`, `resume_session`, event streaming, wait, abort, budget accounting, and cancellation.

This decision does not forbid future direct SDK or HTTP integrations. It says the integration must stay behind the execution boundary and must not pull provider credentials, provider-specific retry state, or vendor protocol details into the orchestrator core. Direct HTTP/SSE is acceptable when it is mature enough to be just another `DispatchEngine` or `LlmProvider` implementation.

The operational rule is: energeia owns dispatch lifecycle; provider adapters own provider transport. The CLI subprocess is a transport choice, not an orchestration concept. That is why the code can support `AgentSdkEngine`, `HttpEngine`, and `HermeneusEngine` without changing DAG execution, session management, QA, or budget behavior.

## Consequences

**Positive:**

- **Credential handling stays out of the orchestrator.** OAuth tokens and local auth state remain in provider tooling where login, refresh, revocation, and account selection already exist. Energeia only injects a configured token when the bridge explicitly supports it, and avoids making API secrets a central dispatch concern.
- **Provider diversity is cheaper.** Claude Code, Codex, Kimi, and future CLIs can be adapted at the process/provider edge while preserving the shared dispatch lifecycle. The orchestration code remains focused on concurrency, cancellation, budgets, health, and outcomes.
- **The boundary supports replacement.** If a native SDK becomes the better transport, the replacement can land behind `DispatchEngine` or `LlmProvider` without changing the DAG, session manager, or backend control plane.
- **Local behavior matches operator workflows.** Provider CLIs already encode local workspace access, account state, permission modes, and provider-specific flags. Reusing them reduces duplicated policy code inside aletheia.

**Negative:**

- **Subprocesses add failure modes.** CLI binaries can be missing, busy, slow to start, or change output format. The code already carries process lifecycle handling, stdout/stderr capture, timeouts, and retries, and those concerns remain part of provider operations.
- **CLI semantics are provider-specific.** Flags such as permission bypasses, stream formats, model names, and working-directory handling differ by provider. Normalization happens in adapters, so each adapter needs focused tests when a provider CLI changes.
- **Streaming fidelity is bounded by CLI output.** Direct SDKs may expose richer structured streaming or usage metadata than a CLI can report. The local-CLI pattern trades some protocol precision for operational stability and credential simplicity.
- **The naming can confuse new readers.** `HttpEngine` and `AgentSdkEngine` describe intended or historical boundaries while currently spawning CLIs. Documentation and ADR references must keep that distinction visible until native transports exist.

## References

- [forkwright/aletheia#4039](https://github.com/forkwright/aletheia/issues/4039) - ADR canary issue requesting ADR-001.
- `crates/energeia/src/engine.rs:27` - `DispatchEngine` session execution boundary.
- `crates/energeia/src/http/client.rs:3` - local Claude CLI transport rationale behind `HttpEngine`.
- `crates/energeia/src/agent_sdk.rs:71` - OAuth-enabled Claude CLI bridge rationale.
- `crates/hermeneus/src/cc/process.rs:134`, `crates/hermeneus/src/codex/process.rs:78`, `crates/hermeneus/src/kimi/process.rs:75` - concrete provider subprocess adapters.
- Michael Nygard, "Documenting Architecture Decisions" - lightweight decision record practice used by this ADR.
