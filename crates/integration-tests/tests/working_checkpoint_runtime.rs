//! Integration coverage for durable working-checkpoint tool storage.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test setup")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use hermeneus::secret::SecretVault;
use koina::id::{NousId, SessionId, ToolName};
use nous::working_memory::FjallWorkingCheckpointStore;
use organon::builtins::working_checkpoint;
use organon::registry::ToolRegistry;
use organon::testing::install_crypto_provider;
use organon::types::{
    ServerToolConfig, ToolContext, ToolGroupId, ToolGroupPolicy, ToolHttpClients, ToolInput,
    ToolServices, WorkingCheckpointStore,
};

fn checkpoint_context(
    store: Arc<dyn WorkingCheckpointStore>,
    session_id: SessionId,
    turn_number: u64,
    workspace: PathBuf,
) -> ToolContext {
    install_crypto_provider();
    ToolContext {
        nous_id: NousId::new("alice").expect("valid synthetic agent id"),
        session_id,
        turn_number,
        workspace: workspace.clone(),
        allowed_roots: vec![workspace],
        services: Some(Arc::new(ToolServices {
            working_checkpoint_store: Some(store),
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: None,
            spawn: None,
            planning: None,
            knowledge: None,
            http_clients: ToolHttpClients {
                general: reqwest::Client::new(),
                ssrf_safe: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            },
            secret_vault: SecretVault::new(),
            lazy_tool_catalog: Vec::new(),
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn checkpoint_input(content: &str) -> ToolInput {
    ToolInput {
        name: ToolName::from_static("update_working_checkpoint"),
        tool_use_id: "toolu_checkpoint_persist".to_owned(),
        arguments: serde_json::json!({ "content": content }),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn update_working_checkpoint_survives_store_reopen() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_path = tmp
        .path()
        .join("instance")
        .join("data")
        .join("working-checkpoints.fjall");
    std::fs::create_dir_all(store_path.parent().unwrap()).expect("create data dir");

    let session_id = SessionId::new();
    let session_key = session_id.to_string();
    let mut registry = ToolRegistry::new();
    working_checkpoint::register(&mut registry).expect("register checkpoint tool");

    {
        let store: Arc<dyn WorkingCheckpointStore> = Arc::new(
            FjallWorkingCheckpointStore::open(&store_path).expect("open checkpoint store"),
        );
        let ctx = checkpoint_context(store, session_id.clone(), 7, tmp.path().to_path_buf());
        let result = registry
            .execute_checked(
                &checkpoint_input("decision: keep build serializer enabled"),
                &ctx,
                "alice",
                &ToolGroupPolicy::groups(vec![ToolGroupId::Edit]),
            )
            .await
            .expect("checkpoint tool executes");
        assert!(
            !result.is_error,
            "checkpoint tool should succeed: {result:?}"
        );
    }

    let reopened = FjallWorkingCheckpointStore::open(&store_path).expect("reopen checkpoint store");
    let checkpoint = reopened
        .read_latest(&session_key)
        .expect("read latest checkpoint")
        .expect("checkpoint persisted");

    assert_eq!(checkpoint.session_id, session_key);
    assert_eq!(checkpoint.turn_number, 7);
    assert_eq!(
        checkpoint.content,
        "decision: keep build serializer enabled"
    );
}
