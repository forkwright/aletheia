//! Shared `StateBuilder` + `issue_token` helpers for the split
//! `public_api_*.rs` integration test binaries.
//!
//! Cargo treats `tests/common/mod.rs` as a non-binary helper module; each
//! `tests/public_api_*.rs` pulls it in with `mod common;`.
//!
//! WHY: extracted from the monolithic `tests/public_api.rs` (1096 lines) to
//! satisfy `RUST/file-too-long`.

#![expect(
    clippy::expect_used,
    reason = "test helpers — panicking on failure is the point"
)]
#![expect(
    dead_code,
    reason = "shared helpers: not every split file uses every helper"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio_util::sync::CancellationToken;

use diaporeia::state::DiaporeiaState;
use hermeneus::provider::ProviderRegistry;
use koina::secret::SecretString;
use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

// WHY: each test must construct an independent state with its own tempdir so
// that tests can run in parallel and in any order. The builder below assembles
// a minimal `DiaporeiaState` with real workspace components (no mocks).
pub struct StateBuilder {
    auth_mode: String,
    none_role: String,
    signing_key: String,
    instance_root: tempfile::TempDir,
    repomix_enabled: bool,
    knowledge_graph_enabled: bool,
    knowledge_store: Option<std::sync::Arc<mneme::knowledge_store::KnowledgeStore>>,
    note_store: Option<std::sync::Arc<dyn organon::types::NoteStore>>,
    blackboard_store: Option<std::sync::Arc<dyn organon::types::BlackboardStore>>,
}

impl StateBuilder {
    pub fn new() -> Self {
        let instance_root = tempfile::tempdir().expect("create instance tempdir");
        Self {
            auth_mode: "token".to_owned(),
            none_role: "readonly".to_owned(),
            signing_key: "integration-test-signing-key-at-least-32-bytes!".to_owned(),
            instance_root,
            repomix_enabled: false,
            knowledge_graph_enabled: false,
            knowledge_store: None,
            note_store: None,
            blackboard_store: None,
        }
    }

    pub fn auth_mode(mut self, mode: &str) -> Self {
        mode.clone_into(&mut self.auth_mode);
        self
    }

    pub fn none_role(mut self, role: &str) -> Self {
        role.clone_into(&mut self.none_role);
        self
    }

    pub fn repomix_enabled(mut self) -> Self {
        self.repomix_enabled = true;
        self
    }

    pub fn knowledge_graph_enabled(mut self) -> Self {
        self.knowledge_graph_enabled = true;
        self
    }

    pub fn knowledge_store(
        mut self,
        store: std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    ) -> Self {
        self.knowledge_store = Some(store);
        self
    }

    pub fn note_store(mut self, store: std::sync::Arc<dyn organon::types::NoteStore>) -> Self {
        self.note_store = Some(store);
        self
    }

    pub fn blackboard_store(
        mut self,
        store: std::sync::Arc<dyn organon::types::BlackboardStore>,
    ) -> Self {
        self.blackboard_store = Some(store);
        self
    }

    pub fn build(self) -> (Arc<DiaporeiaState>, Arc<JwtManager>, tempfile::TempDir) {
        let oikos = Arc::new(Oikos::from_root(self.instance_root.path()));
        let session_store = Arc::new(TokioMutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let provider_registry = Arc::new(ProviderRegistry::new());
        let tool_registry = Arc::new(ToolRegistry::new());

        let nous_manager = Arc::new(NousManager::new(
            Arc::clone(&provider_registry),
            Arc::clone(&tool_registry),
            Arc::clone(&oikos),
            None,
            None,
            Some(Arc::clone(&session_store)),
            #[cfg(feature = "knowledge-store")]
            None,
            Arc::new(vec![]),
            None,
            None,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        ));

        let jwt_config = JwtConfig {
            signing_key: SecretString::from(self.signing_key.clone()),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-diaporeia-tests".to_owned(),
            ..JwtConfig::default()
        };
        let jwt_manager = Arc::new(JwtManager::new(jwt_config.clone()));
        let auth_facade = Arc::new(
            AuthFacade::in_memory(AuthConfig { jwt: jwt_config }).expect("in-memory auth facade"),
        );

        let auth_for_state = if self.auth_mode == "none" {
            None
        } else {
            Some(auth_facade)
        };

        let mut cfg = AletheiaConfig::default();
        if self.repomix_enabled {
            cfg.mcp.repomix.enabled = true;
            cfg.mcp.repomix.max_output_tokens = 10_000;
        }
        if self.knowledge_graph_enabled {
            cfg.mcp.knowledge_graph.enabled = true;
        }
        let config = Arc::new(RwLock::new(cfg));

        let state = Arc::new(DiaporeiaState {
            session_store,
            nous_manager,
            tool_registry,
            oikos,
            auth_facade: auth_for_state,
            start_time: Instant::now(),
            config,
            auth_mode: self.auth_mode,
            none_role: self.none_role,
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: self.knowledge_store,
            note_store: self.note_store,
            blackboard_store: self.blackboard_store,
        });

        (state, jwt_manager, self.instance_root)
    }
}

pub fn issue_token(jwt: &JwtManager, subject: &str, role: Role) -> String {
    jwt.issue_access(subject, role, None)
        .expect("issue test access token")
}

pub fn issue_token_with_nous_id(
    jwt: &JwtManager,
    subject: &str,
    role: Role,
    nous_id: &str,
) -> String {
    jwt.issue_access(subject, role, Some(nous_id))
        .expect("issue scoped test access token")
}
