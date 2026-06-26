# Air-gapped operation

> Status: supported via the OpenAI-compatible provider (#3414, #3424).

Aletheia has no air-gapped mode flag. The capability emerges from config: remove
every cloud provider from `[[providers]]`, declare one or more local
OpenAI-compatible providers, and the runtime never opens an outbound connection
the operator did not authorize. Once `[[providers]]` is non-empty, there is no
implicit legacy Anthropic provider; only the declared list participates in LLM
routing.

The remainder describes the required local LLM stack, a reference `aletheia.toml`
fragment, and the feature set that drops when no Anthropic provider is
registered.

The local Chat Completions-compatible path is separate from the OpenAI/Codex
cloud path, whose target architecture is the OpenAI Responses API provider
tracked separately. Do not use the local
`openai-compatible` recipe as a Codex cloud recommendation.

## Required local LLM stack

Any server speaking OpenAI's `/v1/chat/completions` wire format works. The
OpenAI adapter has been tested against:

| Server      | Status        | Notes                                           |
|-------------|---------------|-------------------------------------------------|
| llama.cpp   | recommended   | `llama-server --host 127.0.0.1 --port 8088`     |
| ollama      | supported     | `ollama serve` - default endpoint `/v1`         |
| vllm        | supported     | `vllm serve MODEL --port 8000`                  |

The adapter tolerates llama.cpp's quirks (missing `code` / `type` on errors,
SSE lines that omit the space after `data:`) so no server-side workaround is
needed.

## Recommended model

Qwen3.5-35B-A3B-Q8_0 on a workstation-class GPU (24 GB VRAM minimum) gives
parity with Haiku-tier cloud models for most agent tasks. Smaller operators
can run Qwen3.5-8B-Q8_0 on consumer hardware with a quality cost.

The exact model is not prescriptive - anything that exposes function calling
over the OpenAI wire format will route correctly.

## Example config

```toml
# No [[providers]] entry with providerType = "anthropic" → no cloud egress.

[[providers]]
name = "local-qwen"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8088/v1"
deploymentTarget = "embedded"
# `apiKeyEnv` is optional; loopback llama.cpp accepts unauthenticated clients.
models = ["Qwen3.5-35B-A3B-Q8_0"]
```

For a mixed cloud + local deployment, keep the Anthropic entry and add the
local one. Exact model matches beat broad provider catch-alls, and
equal-specificity matches use list order, so model IDs should be unique unless
the ordering is intentional.

```toml
[[providers]]
name = "anthropic"
providerType = "anthropic"
deploymentTarget = "cloud"
apiKeyEnv = "ANTHROPIC_API_KEY"
models = ["claude-opus-4-7", "claude-sonnet-4-6", "claude-haiku-4-5"]

[[providers]]
name = "local-qwen"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8088/v1"
deploymentTarget = "embedded"
models = ["Qwen3.5-35B-A3B-Q8_0"]
```

## Feature set in air-gapped mode

Three Anthropic-native features have no OpenAI equivalent and are disabled
for requests routed to OpenAI-compatible providers:

| Feature                | Behavior                                            |
|------------------------|-----------------------------------------------------|
| `cache_control` markers| Dropped. One warning per request carrying the flag. |
| `thinking` budget      | Dropped. One warning per request with `enabled=true`. |
| Server-side tools      | Request is rejected outright with a clear error.    |

Everything else maps cleanly: system prompts, tool definitions, multi-turn
tool_use / tool_result conversations, streaming deltas, stop reasons.

## Deployment targets and factsensitivity

The `deploymentTarget` field on each provider entry classifies where the
traffic terminates. The factsensitivity filter reads this to decide
which facts are allowed to flow to which provider:

| Target       | Trust level                                                    |
|--------------|----------------------------------------------------------------|
| `cloud`      | Public-only. Facts marked sensitive are filtered before send.  |
| `localhosted`| Operator-trusted. Sensitive content flows, PII does not.       |
| `embedded`   | Fully trusted. Every fact the operator would trust to disk.    |

Air-gapped deployments should mark every provider `embedded` (same host) or
`localhosted` (same subnet). Keeping a `cloud` entry alongside is supported
and does not compromise the local-only path - the router picks based on the
requested model ID, not on trust level.

## Observability

Per-provider metrics carry the `name` from the config entry as the
`provider` label. With two providers registered, Prometheus queries can
split cost, latency, and error rate by name - useful for operators who want
to prove that a given model never hit the cloud entry.

## Related issues

- #3424 - OpenAI-compatible provider (this feature)
- #3414 - Air-gapped mode (this document)
- #3410 - Prompt cache sovereignty default (already disabled)
- FactSensitivity filter (consumes `deploymentTarget`)
