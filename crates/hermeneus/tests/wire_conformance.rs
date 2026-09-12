//! Wire-conformance fixture: `aletheia/hermeneus` vs an OpenAI-compatible
//! serving surface.
//!
//! Actions 5 and 6 of
//! `boxes/menos/takeover/aletheia-kernel-logismos-alignment-2026-09-06.md`
//! (kanon-instance, `aletheia#7152` lane -- a planning doc in a sibling
//! repo, cited here for provenance only). Two crates share the name
//! `hermeneus`: this one (aletheia's LLM-provider client, `crates/hermeneus`
//! in this repo) and logismos's server-side crate of the same name, a
//! 47-line scaffold as of `logismos-ro` `ec69f3d` (`crates/hermeneus/src/lib.rs:10`,
//! "Phase 0 scaffold. No functional code yet."). Cite them as
//! `aletheia/hermeneus` (here) vs `logismos/hermeneus` (there) to avoid
//! ambiguity.
//!
//! # What this proves
//!
//! [`OpenAiProvider`] is aletheia's one client for any OpenAI-compatible
//! endpoint -- the incumbent `llama-server --server` today, logismos's
//! planned `/v1/chat/completions` + SSE, `/v1/embeddings`, `/v1/rerank`
//! later (`logismos-ro crates/hermeneus/src/lib.rs:14-30`). This suite is
//! the "explicit consumer conformance" the ownership contract requires
//! before that swap is authorized
//! (`planning/aletheia/phases/aletheia-local-mode-topology.md:28-32`). It
//! only ever issues ordinary client requests an agent turn would also
//! send -- it never starts, stops, retargets, or reconfigures a provider.
//!
//! # Base URLs: three env vars, not one
//!
//! The incumbent splits chat, embeddings, and rerank across three separate
//! `llama-server` processes confirmed live on 2026-09-06: `llama-server@agent`
//! on `:8089` (chat only, `--parallel 1`; `/props` reports `total_slots: 1`),
//! `llama-server@embed` on `:5005` (`--embedding`), `llama-server@rerank` on
//! `:5010` (`--reranking`). `:8089` answers `POST /v1/embeddings` and
//! `POST /v1/rerank` with `501 not_supported_error` ("Start it with
//! `--embeddings`" / "`--reranking`") -- it is NOT a combined endpoint today,
//! whatever an earlier lane note assumed. logismos's planned `hermeneus`
//! unifies all three behind one process
//! (`logismos-ro crates/hermeneus/src/lib.rs:14-18`). So each surface gets
//! its own independently overridable env var, each defaulting to today's
//! box topology; pointing all three at the same URL is exactly the
//! integration-gate condition once logismos ships:
//!
//! - `HERMENEUS_CONFORMANCE_BASE_URL` (chat completions + SSE) -- default
//!   `http://127.0.0.1:8089/v1`
//! - `HERMENEUS_CONFORMANCE_EMBEDDINGS_URL` -- default
//!   `http://127.0.0.1:5005/v1`
//! - `HERMENEUS_CONFORMANCE_RERANK_URL` -- default
//!   `http://127.0.0.1:5010/v1`
//! - `HERMENEUS_CONFORMANCE_MODEL` / `_EMBEDDING_MODEL` / `_RERANK_MODEL` --
//!   the `model` field sent on each request (today's incumbent ignores it;
//!   logismos is expected to route on it once one process serves many
//!   models).
//!
//! Run the exact same suite against logismos once it serves all three:
//!
//! ```text
//! HERMENEUS_CONFORMANCE_BASE_URL=http://127.0.0.1:<port>/v1 \
//! HERMENEUS_CONFORMANCE_EMBEDDINGS_URL=http://127.0.0.1:<port>/v1 \
//! HERMENEUS_CONFORMANCE_RERANK_URL=http://127.0.0.1:<port>/v1 \
//! cargo nextest run -p hermeneus --test wire_conformance
//! ```
//!
//! # Fixtures
//!
//! `tests/fixtures/wire_conformance/*.json` are the exact request/response
//! bodies recorded against the live box on 2026-09-06 (see each file's
//! `captured_at` / `captured_from`). Nothing secret rides on this endpoint
//! -- loopback, no auth configured -- and every fixture was grepped for
//! bearer/api-key/secret-shaped strings before landing; none were found.
//! The `live` tests below re-issue the same requests against whatever
//! `HERMENEUS_CONFORMANCE_*` URL is configured (default: the incumbent) and
//! check the response's *shape* against the fixture (same top-level keys),
//! never byte equality -- ids, timestamps, and token counts are never
//! stable across runs.
//!
//! # Skipping
//!
//! Every `live` test probes its endpoint first ([`probe`]) and, when
//! nothing answers, prints a typed [`SkipReason`] and returns -- it never
//! fails. `cargo test` / `nextest` has no first-class "skipped" outcome;
//! this is the correct shape for "expected in CI, which has no
//! `llama-server`, not a suite failure." The `refusal_mapping` tests need
//! no live endpoint at all: they replay the recorded fixture bodies through
//! a `wiremock` double, so `cargo nextest run -p hermeneus` exercises the
//! refusal-mapping table (Action 6) unconditionally, whether or not the
//! incumbent is reachable.
//!
//! # Refusal mapping (Action 6)
//!
//! For each `Refusal` variant the alignment map names
//! (`aletheia-kernel-logismos-alignment-2026-09-06.md` section 7:
//! `enum Refusal { NotResident, CapacityExceeded, StaleGrant, PrivacyMismatch, Unsupported(Format) }`)
//! plus the incumbent's real refusals, observed at HTTP status + body ->
//! aletheia's typed outcome. `ProviderNotReady` is emitted only by the
//! front door (`crates/aletheia/src/runtime/setup.rs:84-135`,
//! `crates/hermeneus/src/openai/client.rs:121-133`) for a transport-level
//! failure (connection refused / timeout) -- never for an HTTP status the
//! endpoint actually answered with, which always classifies as a hard
//! `Error::ApiError` today unless it happens to be 401/403/429 (auth /
//! rate-limit) or 5xx (server error, retryable):
//!
//! | Refusal | Source | HTTP status + body (recorded) | aletheia outcome | Status |
//! |---|---|---|---|---|
//! | missing `messages` | incumbent, live (`refusal_missing_messages.json`) | 400, `{"error":{"code":400,...}}` -- `code` is a JSON number, so the code-matched branches in `map_error_response` never run; the generic `HTTP {status}: {body}` fallback does | `Error::ApiError { status: 400 }` -- hard, not retryable | real, tested |
//! | malformed request JSON | incumbent, live (`refusal_malformed_json.json`) | 500, `{"error":{"code":500,"type":"server_error",...}}` -- a client-caused mistake reported as a server error | `Error::ApiError { status: 500 }` -- hard, but classifies retryable (any 5xx does, `error.rs::is_retryable`) | real, tested |
//! | connect-refused / timeout (idle-stopped backend) | incumbent shape, transport-level (no fixture -- dynamic, not a captured body) | none (no HTTP response reaches the client) | `Error::ProviderNotReady { state: Sleeping \| Loading \| Failed }` -- hard, not retryable | real, tested |
//! | over-capacity (aletheia's OWN client-side admission cap, today) | in-process, `crates/hermeneus/src/concurrency.rs` | none (refused before any request leaves the process) | `Error::ProviderSaturated { max_running, max_waiting }` -- hard, not retryable; distinct from logismos's own admission | real, tested |
//! | `NotResident` | logismos, wire shape undefined | undefined -- never invented here | today's default for an undefined status: `Error::ApiError { status }` -- hard | **pending logismos** -- see the alignment map section 7 |
//! | `CapacityExceeded` (logismos's authoritative admission refusal) | logismos, wire shape undefined | undefined -- never invented here | same default as above | **pending logismos** -- see the alignment map section 7 |
//! | `StaleGrant` | logismos, wire shape undefined | undefined -- never invented here | same default as above | **pending logismos** -- see the alignment map section 7 |
//! | `PrivacyMismatch` | logismos, wire shape undefined | undefined -- never invented here | same default as above | **pending logismos** -- see the alignment map section 7 |

#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::Duration;

use hermeneus::RetryPolicy;
use hermeneus::concurrency::{AdmissionPolicy, ConcurrencyConfig};
use hermeneus::error::Error;
use hermeneus::front_door::FrontDoorState;
use hermeneus::openai::{OpenAiProvider, OpenAiProviderConfig};
use hermeneus::provider::{DeploymentTarget, LlmProvider};
use hermeneus::types::{CompletionRequest, Content, Message, Role, StopReason};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// --- Fixtures, recorded live against the box on 2026-09-06 -----------------

const CHAT_COMPLETION_FIXTURE: &str =
    include_str!("fixtures/wire_conformance/chat_completion.json");
const EMBEDDINGS_FIXTURE: &str = include_str!("fixtures/wire_conformance/embeddings.json");
const RERANK_FIXTURE: &str = include_str!("fixtures/wire_conformance/rerank.json");
const MODELS_CHAT_FIXTURE: &str = include_str!("fixtures/wire_conformance/models_chat.json");
const REFUSAL_MISSING_MESSAGES_FIXTURE: &str =
    include_str!("fixtures/wire_conformance/refusal_missing_messages.json");
const REFUSAL_MALFORMED_JSON_FIXTURE: &str =
    include_str!("fixtures/wire_conformance/refusal_malformed_json.json");

/// Every fixture file, for the well-formedness smoke test below. Two
/// fixtures (`chat_completion_stream.json`, `models_embeddings.json`,
/// `models_rerank.json`) are recorded as evidence but not replayed by any
/// test -- listed here anyway so a corrupted file still fails loudly.
const ALL_FIXTURES: &[(&str, &str)] = &[
    ("chat_completion.json", CHAT_COMPLETION_FIXTURE),
    (
        "chat_completion_stream.json",
        include_str!("fixtures/wire_conformance/chat_completion_stream.json"),
    ),
    ("embeddings.json", EMBEDDINGS_FIXTURE),
    ("rerank.json", RERANK_FIXTURE),
    ("models_chat.json", MODELS_CHAT_FIXTURE),
    (
        "models_embeddings.json",
        include_str!("fixtures/wire_conformance/models_embeddings.json"),
    ),
    (
        "models_rerank.json",
        include_str!("fixtures/wire_conformance/models_rerank.json"),
    ),
    (
        "refusal_missing_messages.json",
        REFUSAL_MISSING_MESSAGES_FIXTURE,
    ),
    (
        "refusal_malformed_json.json",
        REFUSAL_MALFORMED_JSON_FIXTURE,
    ),
];

#[test]
fn fixtures_are_well_formed_json() {
    for (name, raw) in ALL_FIXTURES {
        let _: serde_json::Value = serde_json::from_str(raw)
            .unwrap_or_else(|e| panic!("fixture {name} is not valid JSON: {e}"));
    }
}

// --- Endpoint configuration --------------------------------------------

const ENV_CHAT_BASE_URL: &str = "HERMENEUS_CONFORMANCE_BASE_URL";
const ENV_EMBEDDINGS_BASE_URL: &str = "HERMENEUS_CONFORMANCE_EMBEDDINGS_URL";
const ENV_RERANK_BASE_URL: &str = "HERMENEUS_CONFORMANCE_RERANK_URL";
const ENV_CHAT_MODEL: &str = "HERMENEUS_CONFORMANCE_MODEL";
const ENV_EMBEDDING_MODEL: &str = "HERMENEUS_CONFORMANCE_EMBEDDING_MODEL";
const ENV_RERANK_MODEL: &str = "HERMENEUS_CONFORMANCE_RERANK_MODEL";

const DEFAULT_CHAT_BASE_URL: &str = "http://127.0.0.1:8089/v1";
const DEFAULT_EMBEDDINGS_BASE_URL: &str = "http://127.0.0.1:5005/v1";
const DEFAULT_RERANK_BASE_URL: &str = "http://127.0.0.1:5010/v1";
const DEFAULT_CHAT_MODEL: &str = "qwen3.8-27b";
const DEFAULT_EMBEDDING_MODEL: &str = "qwen3-embedding-0.6b";
const DEFAULT_RERANK_MODEL: &str = "qwen3-reranker-0.6b";

/// The three independently overridable base URLs plus the model name sent
/// on each surface -- see the module docs for why there are three, not one.
struct Endpoints {
    chat_base_url: String,
    embeddings_base_url: String,
    rerank_base_url: String,
    chat_model: String,
    embedding_model: String,
    rerank_model: String,
}

impl Endpoints {
    fn from_env() -> Self {
        Self {
            chat_base_url: env_or(ENV_CHAT_BASE_URL, DEFAULT_CHAT_BASE_URL),
            embeddings_base_url: env_or(ENV_EMBEDDINGS_BASE_URL, DEFAULT_EMBEDDINGS_BASE_URL),
            rerank_base_url: env_or(ENV_RERANK_BASE_URL, DEFAULT_RERANK_BASE_URL),
            chat_model: env_or(ENV_CHAT_MODEL, DEFAULT_CHAT_MODEL),
            embedding_model: env_or(ENV_EMBEDDING_MODEL, DEFAULT_EMBEDDING_MODEL),
            rerank_model: env_or(ENV_RERANK_MODEL, DEFAULT_RERANK_MODEL),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

// --- Skipping: typed reason, never a failure ----------------------------

/// Why a `live` conformance test skipped rather than ran.
///
/// `cargo test` / `nextest` has no first-class "skipped" outcome; printing
/// this via `Display` and returning early is the correct shape when
/// nothing answers the configured endpoint -- expected in CI (no
/// `llama-server` there) and not a suite failure.
#[derive(Debug)]
enum SkipReason {
    /// `GET {url}` did not succeed within the probe timeout.
    EndpointUnreachable { url: String, detail: String },
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EndpointUnreachable { url, detail } => {
                write!(f, "endpoint unreachable ({url}): {detail}")
            }
        }
    }
}

async fn probe(base_url: &str) -> Result<(), SkipReason> {
    ensure_crypto_provider();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3))
        .build()
        .expect("build probe client");
    let url = format!("{base_url}/models");
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => Err(SkipReason::EndpointUnreachable {
            url,
            detail: format!("HTTP {}", resp.status()),
        }),
        Err(source) => Err(SkipReason::EndpointUnreachable {
            url,
            detail: source.to_string(),
        }),
    }
}

/// Probes `base_url`; on success returns `Some(())`, on failure prints the
/// typed [`SkipReason`] and returns `None` so the caller can `return` the
/// test as skipped rather than failed.
async fn ensure_reachable(base_url: &str) -> Option<()> {
    match probe(base_url).await {
        Ok(()) => Some(()),
        Err(reason) => {
            eprintln!("SKIP: {reason}");
            None
        }
    }
}

// --- Shared helpers -------------------------------------------------------

/// `nextest` runs every test in its own fresh process, so this must run
/// again in each one: `reqwest`'s rustls backend panics on the first
/// `Client::builder().build()` in a process with no `CryptoProvider`
/// installed (`crates/hermeneus/src/openai/client.rs::build_http_client`
/// installs one before every client it builds; this file's two raw
/// `reqwest::Client` builders -- [`probe`] and [`http_client`] -- did not,
/// which is what actually failed all five `live` tests the first time this
/// suite ran, not endpoint reachability). `install_default_provider` is
/// idempotent: a second call in the same process is a harmless `Err`.
fn ensure_crypto_provider() {
    let _ = koina::crypto::install_default_provider();
}

fn http_client() -> reqwest::Client {
    ensure_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build reqwest client")
}

/// A short chat request: enough `max_tokens` for the live 27B's brief
/// chain-of-thought (recorded verbatim in `chat_completion.json` as a
/// non-standard `reasoning_content` field hermeneus's wire types do not
/// model and correctly ignore) to finish before the real answer, small
/// enough to stay well under a second end to end -- see the module docs'
/// "keep the recorded chat case short."
fn short_chat_request(model: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("Reply with exactly the word OK and nothing else.".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 120,
        ..Default::default()
    }
}

/// Asserts every top-level key present in the recorded fixture body is also
/// present in `actual_body`. Extra keys on the live side are fine (a future
/// provider may add extension fields, alignment map section 7); a missing
/// key means the wire shape drifted from what this suite was built against.
fn assert_same_top_level_keys(
    fixture_body: &serde_json::Value,
    actual_body: &serde_json::Value,
    label: &str,
) {
    let fixture_obj = fixture_body
        .as_object()
        .unwrap_or_else(|| panic!("{label}: fixture body is not a JSON object"));
    let actual_obj = actual_body
        .as_object()
        .unwrap_or_else(|| panic!("{label}: live response body is not a JSON object"));
    let missing: Vec<&String> = fixture_obj
        .keys()
        .filter(|k| !actual_obj.contains_key(k.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{label}: live response is missing key(s) {missing:?} present in the 2026-09-06 fixture"
    );
}

// --- Live tests: gate the incumbent today, logismos later -----------------

mod live {
    use super::*;

    #[tokio::test]
    async fn chat_completion_wire_conforms() {
        let env = Endpoints::from_env();
        if ensure_reachable(&env.chat_base_url).await.is_none() {
            return;
        }

        let provider = OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-chat".to_owned(),
            base_url: env.chat_base_url.clone(),
            models: vec![env.chat_model.clone()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            ..Default::default()
        })
        .expect("provider constructs against a loopback/https base_url");

        let response = provider
            .complete(&short_chat_request(&env.chat_model))
            .await
            .expect("chat completion succeeds against a reachable endpoint");

        assert!(
            matches!(
                response.stop_reason,
                StopReason::EndTurn | StopReason::MaxTokens
            ),
            "unexpected stop_reason: {:?}",
            response.stop_reason
        );
        assert!(
            response.usage.input_tokens > 0,
            "expected nonzero prompt tokens"
        );
        assert!(
            !response.model.is_empty(),
            "served model name must be non-empty"
        );
        assert_eq!(
            provider.front_door_state(),
            Some(FrontDoorState::Ready),
            "a successful completion must mark the front door Ready \
             (crates/aletheia/src/runtime/setup.rs:84-135 wires front_door_enabled \
             for every localhosted/embedded provider entry)"
        );
    }

    #[tokio::test]
    async fn chat_completion_streaming_wire_conforms() {
        let env = Endpoints::from_env();
        if ensure_reachable(&env.chat_base_url).await.is_none() {
            return;
        }

        let provider = OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-chat-stream".to_owned(),
            base_url: env.chat_base_url.clone(),
            models: vec![env.chat_model.clone()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            ..Default::default()
        })
        .expect("provider constructs");

        let mut events = Vec::new();
        let response = provider
            .complete_streaming(&short_chat_request(&env.chat_model), &mut |event| {
                events.push(event);
            })
            .await
            .expect("streaming chat completion terminates cleanly on the SSE [DONE] marker");

        assert!(
            !events.is_empty(),
            "expected at least one StreamEvent (MessageStart always fires on the first chunk)"
        );
        assert!(
            matches!(
                response.stop_reason,
                StopReason::EndTurn | StopReason::MaxTokens
            ),
            "unexpected stop_reason: {:?}",
            response.stop_reason
        );
        // WHY: whether a TextDelta appears (vs pure reasoning_content, which
        // hermeneus's SSE parser correctly ignores) is a function of how many
        // tokens the live model's chain-of-thought consumes on this run --
        // stochastic, not part of the wire contract. chat_completion.json
        // records an example run that does produce one. What the wire
        // contract guarantees, and what this asserts, is that the
        // accumulator reaches a terminal state regardless.
        //
        // Usage is deterministically zero here, not stochastic: hermeneus's
        // streaming request never sets `stream_options: {"include_usage":
        // true}` (no such field exists on `ChatCompletionRequest`,
        // `crates/hermeneus/src/openai/wire/request.rs`), and the standard
        // OpenAI streaming contract omits usage from every chunk unless a
        // client opts in -- confirmed 2026-09-06 that this same incumbent
        // DOES return a final `{"choices":[],"usage":{...}}` chunk when a
        // raw request sets that field, so the zero below is hermeneus's own
        // current client-side choice, not an incumbent limitation. Assert
        // the true-today value rather than one hermeneus doesn't request:
        // if a future change wires `stream_options.include_usage`, this
        // assertion trips loudly and should flip to a positive-usage check,
        // the same tripwire pattern `models_listing_wire_conforms` uses for
        // `artifact_sha256` above.
        assert_eq!(
            response.usage.output_tokens, 0,
            "usage.output_tokens is unconditionally 0 while hermeneus's streaming request \
             does not set stream_options.include_usage -- see the WHY above before changing \
             this assertion"
        );
    }

    #[tokio::test]
    async fn models_listing_wire_conforms() {
        let env = Endpoints::from_env();
        if ensure_reachable(&env.chat_base_url).await.is_none() {
            return;
        }

        let client = http_client();
        let response = client
            .get(format!("{}/models", env.chat_base_url))
            .send()
            .await
            .expect("GET /v1/models sends");
        assert!(
            response.status().is_success(),
            "GET /v1/models returned {}",
            response.status()
        );
        let body: serde_json::Value = response.json().await.expect("/v1/models response is JSON");

        let fixture: serde_json::Value =
            serde_json::from_str(MODELS_CHAT_FIXTURE).expect("fixture parses");
        let fixture_body = fixture
            .pointer("/response/body")
            .expect("fixture has response.body");
        assert_same_top_level_keys(fixture_body, &body, "models(chat)");

        let served_id = body
            .pointer("/data/0/id")
            .and_then(serde_json::Value::as_str)
            .expect("data[0].id is a string");
        assert!(!served_id.is_empty(), "served model id must be non-empty");

        // WHY: Action 5 records the artifact identity "if exposed by
        // /v1/models" -- on the incumbent it is NOT (only the
        // llama.cpp-specific /props endpoint carries model_path, outside the
        // OpenAI wire contract this suite gates). If a future endpoint
        // (logismos's ModelRef.artifact_sha256, alignment map section 7)
        // starts exposing it here, this assertion trips loudly rather than
        // silently missing the new identity field.
        assert!(
            body.pointer("/data/0/artifact_sha256").is_none(),
            "an artifact_sha256 field appeared on /v1/models -- update this fixture and the \
             refusal-mapping doc to record the newly exposed artifact identity"
        );
    }

    #[tokio::test]
    async fn embeddings_wire_conforms() {
        let env = Endpoints::from_env();
        if ensure_reachable(&env.embeddings_base_url).await.is_none() {
            return;
        }

        let client = http_client();
        let request_body = serde_json::json!({
            "model": env.embedding_model,
            "input": "hello world",
        });
        let response = client
            .post(format!("{}/embeddings", env.embeddings_base_url))
            .json(&request_body)
            .send()
            .await
            .expect("POST /v1/embeddings sends");
        assert!(
            response.status().is_success(),
            "POST /v1/embeddings returned {}",
            response.status()
        );
        let body: serde_json::Value = response
            .json()
            .await
            .expect("/v1/embeddings response is JSON");

        let fixture: serde_json::Value =
            serde_json::from_str(EMBEDDINGS_FIXTURE).expect("fixture parses");
        let fixture_body = fixture
            .pointer("/response/body")
            .expect("fixture has response.body");
        assert_same_top_level_keys(fixture_body, &body, "embeddings");

        let embedding = body
            .pointer("/data/0/embedding")
            .and_then(serde_json::Value::as_array)
            .expect("data[0].embedding is an array");
        // WHY: episteme::embedding.rs's openai-compat default binds the
        // recall index to 1024 dims (Qwen3-Embedding-0.6B). A dimension
        // drift here is exactly the "changing embedding space requires
        // reindexing" case the alignment map calls out (section 6.2) --
        // this must fail loudly, not silently accept a new size.
        assert_eq!(
            embedding.len(),
            1024,
            "embedding dimension drifted from the 1024-dim recall baseline"
        );
    }

    #[tokio::test]
    async fn rerank_wire_conforms() {
        let env = Endpoints::from_env();
        if ensure_reachable(&env.rerank_base_url).await.is_none() {
            return;
        }

        let client = http_client();
        let request_body = serde_json::json!({
            "model": env.rerank_model,
            "query": "capital of france",
            "documents": ["Paris is the capital of France.", "Bananas are yellow."],
        });
        let response = client
            .post(format!("{}/rerank", env.rerank_base_url))
            .json(&request_body)
            .send()
            .await
            .expect("POST /v1/rerank sends");
        assert!(
            response.status().is_success(),
            "POST /v1/rerank returned {}",
            response.status()
        );
        let body: serde_json::Value = response.json().await.expect("/v1/rerank response is JSON");

        let fixture: serde_json::Value =
            serde_json::from_str(RERANK_FIXTURE).expect("fixture parses");
        let fixture_body = fixture
            .pointer("/response/body")
            .expect("fixture has response.body");
        assert_same_top_level_keys(fixture_body, &body, "rerank");

        let results = body
            .pointer("/results")
            .and_then(serde_json::Value::as_array)
            .expect("results is an array");
        assert_eq!(
            results.len(),
            2,
            "expected one relevance_score per submitted document"
        );
        let top = results
            .first()
            .and_then(|v| v.get("relevance_score"))
            .and_then(serde_json::Value::as_f64)
            .expect("results[0].relevance_score is a number");
        let bottom = results
            .get(1)
            .and_then(|v| v.get("relevance_score"))
            .and_then(serde_json::Value::as_f64)
            .expect("results[1].relevance_score is a number");
        assert!(
            top > bottom,
            "the on-topic document should score above the unrelated one"
        );
    }
}

// --- Refusal mapping (Action 6): deterministic, no live dependency --------

mod refusal_mapping {
    use super::*;

    fn mock_provider(server: &MockServer, admission: Option<AdmissionPolicy>) -> OpenAiProvider {
        OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-refusal".to_owned(),
            base_url: format!("{}/v1", server.uri()),
            models: vec!["qwen3.8-27b".to_owned()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            concurrency: admission.map_or_else(ConcurrencyConfig::default, |policy| {
                ConcurrencyConfig {
                    admission: policy,
                    ..Default::default()
                }
            }),
            ..Default::default()
        })
        .expect("mock provider constructs")
    }

    fn success_body() -> serde_json::Value {
        serde_json::json!({
            "id": "chatcmpl-mock",
            "model": "qwen3.8-27b",
            "choices": [{
                "message": {"role": "assistant", "content": "OK"},
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1, "total_tokens": 11}
        })
    }

    /// Real incumbent refusal, recorded 2026-09-06 as
    /// `refusal_missing_messages.json`: `'messages' is required`.
    /// -> `Error::ApiError { status: 400 }`, a hard error (the endpoint
    /// answered; this is never `ProviderNotReady`, which is reserved for
    /// transport failures).
    #[tokio::test]
    async fn incumbent_missing_messages_is_a_hard_error() {
        let fixture: serde_json::Value =
            serde_json::from_str(REFUSAL_MISSING_MESSAGES_FIXTURE).expect("fixture parses");
        let body = fixture
            .pointer("/response/body")
            .expect("fixture has response.body")
            .clone();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(&body))
            .mount(&server)
            .await;

        let provider = mock_provider(&server, None);
        let err = provider
            .complete(&short_chat_request("qwen3.8-27b"))
            .await
            .expect_err("missing 'messages' must fail");

        assert!(!err.is_retryable(), "a 400 must not be retried: {err}");
        match err {
            Error::ApiError { status, .. } => assert_eq!(status, 400),
            other => panic!("expected a hard ApiError (not ProviderNotReady), got: {other}"),
        }
    }

    /// Real incumbent refusal, recorded 2026-09-06 as
    /// `refusal_malformed_json.json`: malformed request JSON reported as
    /// HTTP 500 `server_error`, not 400 -- a bare status-code check must
    /// not assume 4xx means client error on this server.
    /// -> `Error::ApiError { status: 500 }`, hard but classifies retryable
    /// (`error.rs::is_retryable` treats any 5xx `ApiError` as transient).
    #[tokio::test]
    async fn incumbent_malformed_json_is_a_hard_but_retryable_5xx() {
        let fixture: serde_json::Value =
            serde_json::from_str(REFUSAL_MALFORMED_JSON_FIXTURE).expect("fixture parses");
        let body = fixture
            .pointer("/response/body")
            .expect("fixture has response.body")
            .clone();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_json(&body))
            .mount(&server)
            .await;

        let provider = mock_provider(&server, None);
        let err = provider
            .complete(&short_chat_request("qwen3.8-27b"))
            .await
            .expect_err("a 500 server_error must fail");

        assert!(
            err.is_retryable(),
            "a 5xx ApiError classifies as retryable: {err}"
        );
        match err {
            Error::ApiError { status, .. } => assert_eq!(status, 500),
            other => panic!("expected a hard ApiError (not ProviderNotReady), got: {other}"),
        }
    }

    /// A genuinely unreachable loopback port -- the shape a real
    /// sleeping/idle-stopped on-demand backend produces before its first
    /// successful request. Front-door-enabled providers must surface this
    /// as the typed, non-retryable `ProviderNotReady`, never the generic
    /// transient `ApiRequest` classification (`setup.rs:84-135`,
    /// `openai/client.rs:121-133`). No live endpoint required.
    #[tokio::test]
    async fn transport_failure_maps_to_provider_not_ready_not_a_hard_error() {
        let provider = OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-transport".to_owned(),
            base_url: "http://127.0.0.1:8198/v1".to_owned(),
            models: vec!["qwen3.8-27b".to_owned()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            retry_policy: RetryPolicy {
                max_retries: 0,
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("provider constructs");

        let err = provider
            .complete(&short_chat_request("qwen3.8-27b"))
            .await
            .expect_err("connection refused must fail");

        assert!(
            !err.is_retryable(),
            "ProviderNotReady must not be retried: {err}"
        );
        match err {
            Error::ProviderNotReady { state, .. } => {
                assert_eq!(state, FrontDoorState::Sleeping);
            }
            other => panic!("expected ProviderNotReady, got: {other}"),
        }
    }

    /// The menos-code incident regression: a `--parallel 1` llama.cpp
    /// backend cold-loading (~15s model load) while busy with interactive
    /// turns answers a maintenance-cycle request with an accepted connection
    /// and no timely response -- a client-side timeout, surfaced as the
    /// typed Loading refusal. Loading is deployment lifecycle state, not
    /// failure evidence: no number of consecutive timeouts may latch the
    /// front door Failed. The mock holds the connection open past the
    /// provider's configured request timeout to produce a real reqwest
    /// timeout on every attempt.
    #[tokio::test]
    async fn loading_timeouts_never_latch_the_front_door_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(5))
                    .set_body_json(success_body()),
            )
            .mount(&server)
            .await;

        let provider = OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-loading".to_owned(),
            base_url: format!("{}/v1", server.uri()),
            models: vec!["qwen3.8-27b".to_owned()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            request_timeout: Duration::from_millis(100),
            retry_policy: RetryPolicy {
                max_retries: 0,
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("provider constructs");

        // Well past FRONT_DOOR_FAILURE_THRESHOLD: if timeouts counted
        // toward the latch, the third refusal would come back Failed.
        for attempt in 0..6 {
            let err = provider
                .complete(&short_chat_request("qwen3.8-27b"))
                .await
                .expect_err("a timed-out request must fail");
            match err {
                Error::ProviderNotReady { state, .. } => {
                    assert_eq!(
                        state,
                        FrontDoorState::Loading,
                        "attempt {attempt}: a Loading timeout must never escalate to Failed"
                    );
                }
                other => panic!("expected ProviderNotReady, got: {other}"),
            }
        }
        assert_eq!(
            provider.front_door_state(),
            Some(FrontDoorState::Loading),
            "consecutive Loading timeouts must leave the front door Loading, not Failed"
        );
    }

    /// The latch still works for genuine transport failures: repeated
    /// connection refusals (nothing accepting connections at all) escalate
    /// to `Failed` at `FRONT_DOOR_FAILURE_THRESHOLD`, which is what asks an
    /// operator to look at a down backend instead of retrying forever.
    #[tokio::test]
    async fn connect_refusals_still_latch_the_front_door_failed() {
        let provider = OpenAiProvider::new(OpenAiProviderConfig {
            name: "conformance-refused".to_owned(),
            base_url: "http://127.0.0.1:8197/v1".to_owned(),
            models: vec!["qwen3.8-27b".to_owned()],
            deployment_target: DeploymentTarget::Embedded,
            front_door_enabled: true,
            retry_policy: RetryPolicy {
                max_retries: 0,
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("provider constructs");

        let expected_states = [
            FrontDoorState::Sleeping,
            FrontDoorState::Sleeping,
            FrontDoorState::Failed,
        ];
        for (attempt, expected) in expected_states.iter().enumerate() {
            let err = provider
                .complete(&short_chat_request("qwen3.8-27b"))
                .await
                .expect_err("connection refused must fail");
            match err {
                Error::ProviderNotReady { state, .. } => {
                    assert_eq!(
                        &state, expected,
                        "attempt {attempt}: unexpected front-door state"
                    );
                }
                other => panic!("expected ProviderNotReady, got: {other}"),
            }
        }
        assert_eq!(
            provider.front_door_state(),
            Some(FrontDoorState::Failed),
            "three consecutive connect refusals must latch Failed"
        );
    }

    /// `CapacityExceeded`, aletheia's OWN side today: the client-side Fixed
    /// admission cap (`crates/hermeneus/src/concurrency.rs`,
    /// `crates/aletheia/src/runtime/setup.rs::admission_policy_for_entry`)
    /// refuses a second concurrent request before it ever reaches the wire.
    /// Distinct from logismos's own planned authoritative `CapacityExceeded`
    /// wire refusal, exercised separately below as `pending logismos`.
    #[tokio::test]
    async fn aletheia_client_side_admission_cap_saturates_independently_of_the_wire() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(150))
                    .set_body_json(success_body()),
            )
            .mount(&server)
            .await;

        let provider = mock_provider(
            &server,
            Some(AdmissionPolicy::Fixed {
                max_running: 1,
                max_waiting: 0,
            }),
        );
        let request = short_chat_request("qwen3.8-27b");

        let (first, second) =
            tokio::join!(provider.complete(&request), provider.complete(&request));

        first.expect("the admitted request succeeds");
        let err = second.expect_err("the second concurrent request must be refused, not queued");
        assert!(
            !err.is_retryable(),
            "ProviderSaturated must not be retried: {err}"
        );
        match err {
            Error::ProviderSaturated {
                max_running,
                max_waiting,
                ..
            } => {
                assert_eq!(max_running, 1);
                assert_eq!(max_waiting, 0);
            }
            other => panic!("expected ProviderSaturated, got: {other}"),
        }
    }

    /// Placeholder HTTP status for logismos's not-yet-defined `Refusal`
    /// wire shapes (alignment map section 7). 409 is NOT a claim about what
    /// logismos will send -- no PR, issue, or doc in either tree defines
    /// that status yet, and none is invented here. It exists only so each
    /// `pending logismos` test below exercises TODAY's real fallthrough
    /// behavior in `map_error_response` for any status outside the
    /// already-special-cased 401/403/429/5xx band.
    const PENDING_LOGISMOS_PLACEHOLDER_STATUS: u16 = 409;

    /// One test per `Refusal` variant named in the alignment map, per
    /// Action 6. Each encodes aletheia's CURRENT default mapping for an
    /// undefined status -- never logismos's actual wire shape, which does
    /// not exist yet. See
    /// `aletheia-kernel-logismos-alignment-2026-09-06.md` section 6 point 3
    /// and section 7 (kanon-instance, `boxes/menos/takeover/`).
    async fn assert_pending_logismos_refusal_is_a_hard_error(refusal_name: &str) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(PENDING_LOGISMOS_PLACEHOLDER_STATUS).set_body_json(
                    serde_json::json!({
                        "error": {
                            "message": format!(
                                "{refusal_name} -- logismos wire shape not yet defined; see \
                                 aletheia-kernel-logismos-alignment-2026-09-06.md section 7"
                            ),
                            "type": "pending_logismos",
                        }
                    }),
                ),
            )
            .mount(&server)
            .await;

        let provider = mock_provider(&server, None);
        let err = provider
            .complete(&short_chat_request("qwen3.8-27b"))
            .await
            .expect_err("pending-logismos placeholder refusal must fail");

        assert!(
            !err.is_retryable(),
            "{refusal_name}: a non-5xx ApiError must not be retryable: {err}"
        );
        match err {
            Error::ApiError { status, .. } => {
                assert_eq!(status, PENDING_LOGISMOS_PLACEHOLDER_STATUS);
            }
            other => panic!(
                "{refusal_name}: expected today's default hard ApiError (never ProviderNotReady \
                 -- that is reserved for transport failures, not HTTP-level refusals), got: {other}"
            ),
        }
    }

    #[tokio::test]
    async fn not_resident_pending_logismos() {
        assert_pending_logismos_refusal_is_a_hard_error("NotResident").await;
    }

    #[tokio::test]
    async fn capacity_exceeded_wire_refusal_pending_logismos() {
        assert_pending_logismos_refusal_is_a_hard_error("CapacityExceeded").await;
    }

    #[tokio::test]
    async fn stale_grant_pending_logismos() {
        assert_pending_logismos_refusal_is_a_hard_error("StaleGrant").await;
    }

    #[tokio::test]
    async fn privacy_mismatch_pending_logismos() {
        assert_pending_logismos_refusal_is_a_hard_error("PrivacyMismatch").await;
    }
}
