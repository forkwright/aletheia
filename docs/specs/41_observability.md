# Spec 41: Observability — Logs, Metrics, Traces, Alerts

**Status:** Draft
**Origin:** Issue #296
**Module:** Cross-cutting

---

## Problem

Currently: structured logs exist (tracing to file), no metrics, no distributed traces, no alerting. No way to answer "why was that turn slow?" or "which agent is failing most?" without manual log grep.

## Three Pillars

### 1. Logs (exists, needs structure)

Current: `createLogger("module")` emits structured JSON to file.

Gaps:
- No log aggregation — logs live on filesystem, no cross-time search
- No log retention policy — files grow unbounded
- No alert on ERROR/WARN patterns

Target: consistent structured fields on every log event (sessionId, turnId, agentId, model, tokens, duration, toolsUsed). Log rotation with maxSize/maxFiles.

### 2. Metrics (new)

Key metrics to instrument:
- Turn latency (p50, p95, p99) by agent and model
- Token usage by agent, model, session
- Tool execution count and duration
- Memory recall latency and hit rate
- Provider error rates (429s, 5xx, timeouts)
- Active sessions and message throughput

### 3. Traces (new)

Distributed tracing across turn lifecycle:
- Pre-turn (context assembly, recall) → LLM call → tool execution → post-turn (memory, distill)
- Langfuse SDK integration (see #277) as the trace backend
- Span hierarchy matching the actual execution flow

## Alerting

- Provider error rate > threshold → credential failover notification
- Turn latency > 60s → investigation signal
- Memory sidecar unreachable → prosoche escalation

## Phases

1. Structured log standardization + rotation
2. Langfuse SDK integration (#277)
3. Metrics instrumentation (key counters and histograms)
4. Alert rules and notification routing
5. Dashboard (Langfuse UI or custom)

## Dependencies

- Langfuse instance deployment (self-hosted or cloud)
- Provider adapter interface (Spec 38) for provider-level metrics
