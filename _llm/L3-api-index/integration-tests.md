# L3 API Index: integration-tests

Crate path: `crates/integration-tests`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/harness.rs`

> Nous id used by the shared integration-test fixture.
```rust
pub const TEST_NOUS_ID: &str = "test-nous";
```

> Session key used by generic helper-created sessions.
```rust
pub const DEFAULT_SESSION_KEY: &str = "e2e-test";
```

> Dimension used by the test embedding provider and knowledge store.
```rust
pub const TEST_EMBEDDING_DIM: usize = 384;
```

> Shared integration-test harness around a pylon `AppState`.
```rust
pub struct TestHarness {
    /// Shared pylon application state.
    pub state: Arc<AppState>,
    /// JWT issuer used by auth helper methods.
    pub jwt_manager: Arc<JwtManager>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    _tmp: tempfile::TempDir,
}
```

```rust
impl TestHarness {
    pub async fn build_minimal () -> Self;
    pub async fn build () -> Self;
    pub async fn build_with_knowledge_store () -> Self;
    pub async fn build_with_provider (provider: Box<dyn LlmProvider>) -> Self;
    pub async fn build_with_provider_and_tools (
        provider: Box<dyn LlmProvider>,
        register_tools: bool,
    ) -> Self;
    pub async fn build_with_provider_and_knowledge_store (provider: Box<dyn LlmProvider>) -> Self;
    pub fn knowledge_store (&self) -> Arc<KnowledgeStore>;
    pub fn embedding_provider (&self) -> Arc<dyn EmbeddingProvider>;
    pub fn auth_token (&self) -> String;
    pub fn router (&self) -> axum::Router;
    pub fn router_with_security (&self, security: &pylon::security::SecurityConfig) -> axum::Router;
    pub fn authed_request (
        &self,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body>;
    pub fn authed_request_with_token (
        &self,
        token: &str,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body>;
    pub fn authed_get (&self, uri: &str) -> Request<Body>;
    pub async fn create_session (&self, router: &axum::Router) -> serde_json::Value;
    pub async fn create_session_with_key (
        &self,
        router: &axum::Router,
        key: &str,
    ) -> serde_json::Value;
    pub async fn send_message (
        &self,
        router: &axum::Router,
        session_id: &str,
        content: &str,
    ) -> String;
    pub async fn get_history (&self, router: &axum::Router, session_id: &str) -> serde_json::Value;
    pub async fn start_tcp_server (self) -> (String, String, Self);
}
```

> Parse an Axum response body as JSON.
```rust
pub async fn body_json (response: axum::response::Response) -> serde_json::Value
```

> Parse an Axum response body as UTF-8 text.
```rust
pub async fn body_string (response: axum::response::Response) -> String
```

## `tests/r722_substrate_canary.rs`

> Create a temp directory with a minimal oikos layout for the given agent.
```rust
pub fn temp_oikos (agent_id: &str) -> (tempfile::TempDir, Arc<Oikos>)
```

> Build a synthetic fact for canary fixtures.
```rust
pub fn make_test_fact (id: &str, nous_id: &str, content: &str) -> Fact
```

> Build a [`ScoredResult`] for recall-stage tests.
```rust
pub fn make_scored_result (
        source_id: &str,
        visibility: Visibility,
        scope: Option<MemoryScope>,
        result_score: f64,
    ) -> ScoredResult
```

> Shared mock provider that captures requests via an external Arc.
```rust
pub struct CapturingMockProvider {
        response: CompletionResponse,
        captured: Arc<Mutex<Vec<CompletionRequest>>>,
    }
```

```rust
impl CapturingMockProvider {
    pub fn new (text: &str, captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self;
}
```

> Build a [`CompletionResponse`] containing a single text block.
```rust
pub fn text_response (text: &str) -> CompletionResponse
```

> No-op tool executor for tests.
```rust
pub struct NoopExecutor;
```

> Build a [`ToolDef`] with the given name and groups.
```rust
pub fn make_tool_def (name: &str, groups: Vec<ToolGroupId>) -> ToolDef
```

> Return a vec of `(skill_json, is_always)` for the always-vs-lazy canary.
> 
> All skills share the "canary" domain tag so that a single BM25 query
> can retrieve the full fixture set for end-to-end verification.
```rust
pub fn sample_skills_fixture () -> Vec<(String, bool)>
```
