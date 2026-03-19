//! Integration tests for thesauros domain packs: loading, bootstrap injection, tool registration.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::types::{
    CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
};
use aletheia_koina::id::ToolName;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolCategory;
use aletheia_taxis::oikos::Oikos;
use aletheia_thesauros::loader::load_packs;
use aletheia_thesauros::tools::register_pack_tools;

// --- Test infrastructure ---

struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_test".to_owned(),
                model: "mock-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from mock!".to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Usage::default()
                },
            },
            captured,
        }
    }
}

impl LlmProvider for CapturingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            Ok(self.response.clone())
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-capturing"
    }
}

fn setup_oikos(dir: &Path, agent_id: &str) -> Arc<Oikos> {
    std::fs::create_dir_all(dir.join(format!("nous/{agent_id}"))).expect("mkdir nous");
    std::fs::create_dir_all(dir.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(dir.join("theke")).expect("mkdir theke");
    std::fs::write(
        dir.join(format!("nous/{agent_id}/SOUL.md")),
        "I am a test agent.",
    )
    .expect("write SOUL.md");
    Arc::new(Oikos::from_root(dir))
}

fn setup_pack(dir: &Path, toml_content: &str, files: &[(&str, &str)]) {
    std::fs::write(dir.join("pack.toml"), toml_content).expect("write pack.toml");
    for (name, content) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, content).expect("write file");
    }
}

fn capturing_providers() -> (Arc<ProviderRegistry>, Arc<Mutex<Vec<CompletionRequest>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(CapturingMockProvider::new(Arc::clone(&captured))));
    (Arc::new(registry), captured)
}

// --- Tests ---

#[tokio::test]
async fn pack_sections_appear_in_bootstrap() {
    let oikos_dir = tempfile::TempDir::new().expect("tmpdir");
    let pack_dir = tempfile::TempDir::new().expect("tmpdir");

    let oikos = setup_oikos(oikos_dir.path(), "test-agent");
    setup_pack(
        pack_dir.path(),
        r#"
name = "test-pack"
version = "1.0"

[[context]]
path = "context/DOMAIN_KNOWLEDGE.md"
priority = "important"
"#,
        &[(
            "context/DOMAIN_KNOWLEDGE.md",
            "Engagements flow into cases which flow into journeys.",
        )],
    );

    let packs = load_packs(&[pack_dir.path().to_path_buf()]);
    assert_eq!(packs.len(), 1);

    let (providers, captured) = capturing_providers();
    let tools = Arc::new(ToolRegistry::new());

    let mut manager = NousManager::new(
        providers,
        tools,
        Arc::clone(&oikos),
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(packs),
        None,
        None,
    );

    let config = NousConfig {
        id: "test-agent".to_owned(),
        model: "mock-model".to_owned(),
        ..NousConfig::default()
    };
    let handle = manager.spawn(config, PipelineConfig::default()).await;

    handle.send_turn("main", "Hello").await.expect("turn");

    {
        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let requests = captured.lock().expect("lock poisoned");
        assert_eq!(requests.len(), 1);
        let system = requests[0].system.as_ref().expect("system prompt");
        assert!(
            system.contains("Engagements flow into cases which flow into journeys."),
            "pack section content should appear in system prompt"
        );
    }

    manager.shutdown_all().await;
}

#[test]
fn pack_tools_registered_and_available() {
    let pack_dir = tempfile::TempDir::new().expect("tmpdir");

    setup_pack(
        pack_dir.path(),
        r#"
name = "tool-pack"
version = "1.0"

[[tools]]
name = "echo_input"
description = "Echoes JSON input back"
command = "tools/echo.sh"

[tools.input_schema]
required = ["message"]

[tools.input_schema.properties.message]
type = "string"
description = "Message to echo"
"#,
        &[("tools/echo.sh", "#!/bin/sh\ncat")],
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(pack_dir.path().join("tools/echo.sh"), perms).expect("chmod");
    }

    let packs = load_packs(&[pack_dir.path().to_path_buf()]);
    assert_eq!(packs.len(), 1);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&packs, &mut registry);
    assert!(errors.is_empty(), "no registration errors: {errors:?}");

    let tool_name = ToolName::new("echo_input").expect("valid name");
    let tool = registry.get_def(&tool_name).expect("tool registered");
    assert_eq!(tool.category, ToolCategory::Domain);
    assert_eq!(tool.description, "Echoes JSON input back");
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test requires two full agent setups"
)]
async fn domain_tagged_sections_reach_correct_agents() {
    let oikos_dir = tempfile::TempDir::new().expect("tmpdir");
    let pack_dir = tempfile::TempDir::new().expect("tmpdir");

    // Create oikos dirs for both agents
    let root = oikos_dir.path();
    std::fs::create_dir_all(root.join("nous/chiron")).expect("mkdir");
    std::fs::create_dir_all(root.join("nous/hermes")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    std::fs::write(root.join("nous/chiron/SOUL.md"), "I am Chiron.").expect("write");
    std::fs::write(root.join("nous/hermes/SOUL.md"), "I am Hermes.").expect("write");
    let oikos = Arc::new(Oikos::from_root(root));

    setup_pack(
        pack_dir.path(),
        r#"
name = "domain-pack"
version = "1.0"

[[context]]
path = "context/GENERAL.md"

[[context]]
path = "context/HEALTHCARE.md"
agents = ["healthcare"]

[overlays.chiron]
domains = ["healthcare"]
"#,
        &[
            ("context/GENERAL.md", "General knowledge for all agents."),
            (
                "context/HEALTHCARE.md",
                "HIPAA compliance requires encryption at rest.",
            ),
        ],
    );

    let packs = load_packs(&[pack_dir.path().to_path_buf()]);

    // Chiron: domains from pack overlay include "healthcare"
    let chiron_captured = Arc::new(Mutex::new(Vec::new()));
    let hermes_captured = Arc::new(Mutex::new(Vec::new()));

    // Spawn chiron (with healthcare domain)
    let mut chiron_providers = ProviderRegistry::new();
    chiron_providers.register(Box::new(CapturingMockProvider::new(Arc::clone(
        &chiron_captured,
    ))));

    let mut chiron_manager = NousManager::new(
        Arc::new(chiron_providers),
        Arc::new(ToolRegistry::new()),
        Arc::clone(&oikos),
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(packs.clone()),
        None,
        None,
    );

    let chiron_config = NousConfig {
        id: "chiron".to_owned(),
        model: "mock-model".to_owned(),
        domains: vec!["healthcare".to_owned()],
        ..NousConfig::default()
    };
    let chiron_handle = chiron_manager
        .spawn(chiron_config, PipelineConfig::default())
        .await;
    chiron_handle
        .send_turn("main", "Hello")
        .await
        .expect("turn");

    // Spawn hermes (no domains)
    let mut hermes_providers = ProviderRegistry::new();
    hermes_providers.register(Box::new(CapturingMockProvider::new(Arc::clone(
        &hermes_captured,
    ))));

    let mut hermes_manager = NousManager::new(
        Arc::new(hermes_providers),
        Arc::new(ToolRegistry::new()),
        Arc::clone(&oikos),
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(packs),
        None,
        None,
    );

    let hermes_config = NousConfig {
        id: "hermes".to_owned(),
        model: "mock-model".to_owned(),
        ..NousConfig::default()
    };
    let hermes_handle = hermes_manager
        .spawn(hermes_config, PipelineConfig::default())
        .await;
    hermes_handle
        .send_turn("main", "Hello")
        .await
        .expect("turn");

    // Verify chiron gets healthcare content
    {
        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let chiron_reqs = chiron_captured.lock().expect("lock poisoned");
        let chiron_system = chiron_reqs[0].system.as_ref().expect("system");
        assert!(
            chiron_system.contains("HIPAA compliance"),
            "chiron (healthcare domain) should see healthcare section"
        );
        assert!(
            chiron_system.contains("General knowledge"),
            "chiron should see general section"
        );
    }

    // Verify hermes does NOT get healthcare content
    {
        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let hermes_reqs = hermes_captured.lock().expect("lock poisoned");
        let hermes_system = hermes_reqs[0].system.as_ref().expect("system");
        assert!(
            !hermes_system.contains("HIPAA compliance"),
            "hermes (no healthcare domain) should not see healthcare section"
        );
        assert!(
            hermes_system.contains("General knowledge"),
            "hermes should see general section"
        );
    }

    chiron_manager.shutdown_all().await;
    hermes_manager.shutdown_all().await;
}

#[test]
fn missing_pack_warns_not_crashes() {
    let good_dir = tempfile::TempDir::new().expect("tmpdir");
    setup_pack(
        good_dir.path(),
        "name = \"good-pack\"\nversion = \"1.0\"\n",
        &[],
    );

    let packs = load_packs(&[
        std::path::PathBuf::from("/nonexistent/pack/path"),
        good_dir.path().to_path_buf(),
    ]);

    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].manifest.name, "good-pack");
}

#[test]
fn invalid_manifest_skips_gracefully() {
    let bad_dir = tempfile::TempDir::new().expect("tmpdir");
    std::fs::write(
        bad_dir.path().join("pack.toml"),
        "this = [is = not = valid = toml {{}}",
    )
    .expect("write bad toml");

    let packs = load_packs(&[bad_dir.path().to_path_buf()]);
    assert!(packs.is_empty());
}
