//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, RouteDecision, reply_target};
use agora::types::{InboundMessage, SendParams};
use mneme::store::SessionStore;
use nous::manager::NousManager;

use self::command_record::{
    CommandOutcome, CommandRecordInput, CommandRecordStart, begin_command_record,
    finish_command_record,
};

mod command_record;

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
/// Per-message dispatch tasks are tracked in a `JoinSet` and drained on exit.
pub(crate) fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    session_store: Arc<Mutex<SessionStore>>,
    mut ready_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let span = tracing::info_span!("message_dispatcher");
    tokio::spawn(
        async move {
            while !*ready_rx.borrow_and_update() {
                if ready_rx.changed().await.is_err() {
                    warn!("ready channel dropped before ready signal");
                    return;
                }
            }
            info!("dispatch loop started");

            let mut in_flight = JoinSet::new();

            while let Some(msg) = rx.recv().await {
                let router = Arc::clone(&router);
                let nous_mgr = Arc::clone(&nous_manager);
                let channels = Arc::clone(&channel_registry);
                let store = Arc::clone(&session_store);
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %msg.sender,
                );
                in_flight.spawn(
                    dispatch_one(msg, router, nous_mgr, channels, store).instrument(msg_span),
                );

                // WHY: Reap completed tasks periodically to prevent unbounded growth.
                while let Some(result) = in_flight.try_join_next() {
                    if let Err(e) = result {
                        warn!(error = %e, "dispatch task panicked");
                    }
                }
            }

            // WHY: Drain remaining in-flight dispatch tasks before exiting.
            info!(
                remaining = in_flight.len(),
                "dispatch loop draining in-flight tasks"
            );
            while let Some(result) = in_flight.join_next().await {
                if let Err(e) = result {
                    warn!(error = %e, "dispatch task panicked during drain");
                }
            }

            info!("dispatch loop stopped");
        }
        .instrument(span),
    )
}

async fn dispatch_one(
    msg: InboundMessage,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    session_store: Arc<Mutex<SessionStore>>,
) {
    let Some(decision) = router.resolve(&msg) else {
        warn!(
            channel = %msg.channel,
            sender = %msg.sender,
            "no route for inbound message, dropping"
        );
        return;
    };

    // NOTE: `!`-commands are intercepted before reaching the nous agent.
    // Plain turns fall through to send_turn as before.
    if let Some(cmd) = command::parse(&msg.text) {
        handle_command_message(
            &msg,
            &cmd,
            &decision,
            &nous_manager,
            &channel_registry,
            &session_store,
        )
        .await;
        return;
    }

    let Some(handle) = nous_manager.get(decision.nous_id) else {
        warn!(
            nous_id = %decision.nous_id,
            "routed to unknown nous actor, dropping"
        );
        return;
    };

    info!(
        nous_id = %decision.nous_id,
        session_key = %decision.session_key,
        matched_by = ?decision.matched_by,
        "dispatching turn"
    );

    let turn_result = match handle.send_turn(&decision.session_key, &msg.text).await {
        Ok(result) => result,
        Err(e) => {
            warn!(error = %e, nous_id = %decision.nous_id, "turn failed");
            return;
        }
    };

    send_reply(&msg, &turn_result.content, &channel_registry).await;
}

async fn handle_command_message(
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
    nous_manager: &NousManager,
    channel_registry: &ChannelRegistry,
    session_store: &Mutex<SessionStore>,
) {
    let input = CommandRecordInput::from_message(
        msg,
        decision.nous_id,
        &decision.session_key,
        cmd,
        nous_manager
            .get_config(decision.nous_id)
            .map(|config| config.generation.model.as_str()),
    );
    let command_start = match begin_command_record(session_store, &input).await {
        Ok(start) => Some(start),
        Err(e) => {
            warn!(
                error = %e,
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "failed to persist command lifecycle start"
            );
            None
        }
    };
    if let Some(CommandRecordStart::Duplicate { reply_text }) = command_start {
        info!(
            nous_id = %decision.nous_id,
            session_key = %decision.session_key,
            command = cmd.name(),
            "replaying duplicate !-command response"
        );
        send_reply(msg, &reply_text, channel_registry).await;
        return;
    }
    if let Some(CommandRecordStart::InFlight { reply_text }) = command_start {
        info!(
            nous_id = %decision.nous_id,
            session_key = %decision.session_key,
            command = cmd.name(),
            "duplicate !-command is already in flight"
        );
        send_reply(msg, &reply_text, channel_registry).await;
        return;
    }
    let session_id = command_start.and_then(|start| match start {
        CommandRecordStart::New { session_id } => Some(session_id),
        CommandRecordStart::Duplicate { .. } | CommandRecordStart::InFlight { .. } => None,
    });

    debug!(
        nous_id = %decision.nous_id,
        command = cmd.name(),
        "dispatching !-command"
    );
    let started = std::time::Instant::now();
    let reply_text = execute_command(
        cmd,
        decision.nous_id,
        &decision.session_key,
        nous_manager,
        channel_registry,
    )
    .await;
    if let Some(session_id) = session_id {
        let outcome = CommandOutcome::from_command(cmd, started.elapsed());
        if let Err(e) =
            finish_command_record(session_store, &input, &session_id, &reply_text, outcome).await
        {
            warn!(
                error = %e,
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "failed to persist command lifecycle result"
            );
        }
    }
    send_reply(msg, &reply_text, channel_registry).await;
}

/// Build a `CommandContext` and execute a parsed command, returning the reply text.
#[expect(
    clippy::too_many_lines,
    reason = "command snapshot assembly stays local to dispatch"
)]
async fn execute_command(
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    nous_manager: &NousManager,
    channel_registry: &ChannelRegistry,
) -> String {
    // Gather current-agent snapshot.
    let current_agent = if let Some(handle) = nous_manager.get(nous_id) {
        match handle.status().await {
            Ok(st) => {
                let model = nous_manager
                    .get_config(nous_id)
                    .map_or_else(String::new, |c| c.generation.model.clone());
                let thinking_enabled = nous_manager
                    .get_config(nous_id)
                    .is_some_and(|c| c.generation.thinking_enabled);
                let thinking_budget = nous_manager
                    .get_config(nous_id)
                    .map_or(0, |c| c.generation.thinking_budget);
                Some(AgentSnapshot {
                    id: st.id,
                    lifecycle: st.lifecycle.to_string(),
                    session_count: st.session_count,
                    active_session: st.active_session,
                    panic_count: st.panic_count,
                    uptime_secs: st.uptime.as_secs(),
                    model,
                    thinking_enabled,
                    thinking_budget,
                })
            }
            Err(e) => {
                warn!(error = %e, nous_id, "failed to query agent status for command");
                None
            }
        }
    } else {
        None
    };

    // Gather all-agents snapshot.
    let all_agents = {
        let statuses = nous_manager.list().await;
        statuses
            .into_iter()
            .map(|st| {
                let model = nous_manager
                    .get_config(&st.id)
                    .map_or_else(String::new, |c| c.generation.model.clone());
                let thinking_enabled = nous_manager
                    .get_config(&st.id)
                    .is_some_and(|c| c.generation.thinking_enabled);
                let thinking_budget = nous_manager
                    .get_config(&st.id)
                    .map_or(0, |c| c.generation.thinking_budget);
                AgentSnapshot {
                    id: st.id,
                    lifecycle: st.lifecycle.to_string(),
                    session_count: st.session_count,
                    active_session: st.active_session,
                    panic_count: st.panic_count,
                    uptime_secs: st.uptime.as_secs(),
                    model,
                    thinking_enabled,
                    thinking_budget,
                }
            })
            .collect()
    };

    // Gather channel health snapshots only for commands that need them.
    let channels = match cmd {
        command::Command::Channels => channel_registry
            .probe_all()
            .await
            .into_iter()
            .map(|(id, probe)| ChannelSnapshot {
                id,
                healthy: probe.ok,
                latency_ms: probe.latency_ms,
            })
            .collect(),
        _ => vec![],
    };

    #[cfg(feature = "recall")]
    let skills: Vec<String> = {
        let store = nous_manager
            .get_config(nous_id)
            .and_then(|cfg| nous_manager.knowledge_store_for_cohort(cfg.episteme_cohort.as_ref()));
        match store {
            Some(knowledge_store) => match knowledge_store.find_skills_for_nous(nous_id, 50) {
                Ok(facts) => facts
                    .iter()
                    .map(|fact| {
                        serde_json::from_str::<mneme::skill::SkillContent>(&fact.content)
                            .map_or_else(|_| fact.id.to_string(), |skill| skill.name)
                    })
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "failed to load skills for nous");
                    Vec::new()
                }
            },
            None => Vec::new(),
        }
    };
    #[cfg(not(feature = "recall"))]
    let skills: Vec<String> = Vec::new();

    let blackboard_entries: Vec<String> = match nous_manager.blackboard_store() {
        Some(blackboard_store) => match blackboard_store.list() {
            Ok(entries) => entries
                .iter()
                .map(|entry| {
                    format!(
                        "[{}] = {} (by {})",
                        entry.key, entry.value, entry.author_nous_id
                    )
                })
                .collect(),
            Err(e) => {
                warn!(error = %e, "failed to list blackboard entries");
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    let ctx = CommandContext {
        current_nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        current_agent,
        all_agents,
        skills,
        blackboard_entries,
        channels,
    };

    command::execute(cmd, &ctx)
}

/// Send a reply back through the originating channel.
async fn send_reply(msg: &InboundMessage, text: &str, channel_registry: &ChannelRegistry) {
    let to = reply_target(msg);
    let params = SendParams {
        to,
        text: text.to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    match channel_registry.send(&msg.channel, &params).await {
        Ok(result) => {
            if !result.sent {
                warn!(
                    error = result.error.as_deref().unwrap_or("unknown"),
                    "failed to send reply"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "channel send error");
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    #[cfg(feature = "recall")]
    use std::collections::HashMap;

    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use mneme::store::SessionStore;
    use nous::adapters::SessionBlackboardAdapter;
    use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
    use nous::manager::NousManager;
    use organon::registry::ToolRegistry;
    use organon::types::{BlackboardStore, ToolHttpClients, ToolServices};
    use taxis::oikos::Oikos;

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes temp files synchronously"
    )]
    fn make_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("nous/alice")).expect("create alice workspace");
        std::fs::create_dir_all(root.join("shared")).expect("create shared");
        std::fs::create_dir_all(root.join("theke")).expect("create theke");
        std::fs::write(root.join("nous/alice/SOUL.md"), "I am Alice.").expect("write soul");
        (dir, Arc::new(Oikos::from_root(&root)))
    }

    fn make_providers() -> Arc<ProviderRegistry> {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::new("Hello!").models(&["test-model"]),
        ));
        Arc::new(providers)
    }

    fn make_tool_services(session_store: &Arc<Mutex<SessionStore>>) -> Arc<ToolServices> {
        let blackboard_store: Arc<dyn BlackboardStore> =
            Arc::new(SessionBlackboardAdapter(Arc::clone(session_store)));
        Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: Some(blackboard_store),
            spawn: None,
            planning: None,
            knowledge: None,
            working_checkpoint_store: None,
            http_clients: ToolHttpClients {
                general: reqwest::Client::new(),
                ssrf_safe: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            },
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: Vec::new(),
            server_tool_config: organon::types::ServerToolConfig::default(),
        })
    }

    fn make_config() -> NousConfig {
        NousConfig {
            id: Arc::from("alice"),
            generation: NousGenerationConfig {
                model: "test-model".to_owned(),
                ..NousGenerationConfig::default()
            },
            workspace: PathBuf::from("nous/alice"),
            ..NousConfig::default()
        }
    }

    #[cfg(feature = "recall")]
    fn make_dispatch_manager(
        oikos: Arc<Oikos>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> NousManager {
        use mneme::knowledge_store::KnowledgeStore;

        let mut knowledge_stores = HashMap::new();
        knowledge_stores.insert(
            "shared".to_owned(),
            KnowledgeStore::open_mem().expect("open in-memory knowledge store"),
        );

        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Some(knowledge_stores),
            Arc::new(Vec::new()),
            None,
            tool_services,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(not(feature = "recall"))]
    fn make_dispatch_manager(
        oikos: Arc<Oikos>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> NousManager {
        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Arc::new(Vec::new()),
            None,
            tool_services,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(feature = "recall")]
    fn make_skill_manager(
        oikos: Arc<Oikos>,
        knowledge_stores: HashMap<String, Arc<mneme::knowledge_store::KnowledgeStore>>,
    ) -> NousManager {
        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Some(knowledge_stores),
            Arc::new(Vec::new()),
            None,
            None,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(feature = "recall")]
    fn make_skill_fact(skill_name: &str) -> mneme::knowledge::Fact {
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility, far_future,
        };

        let content = serde_json::to_string(&mneme::skill::SkillContent {
            name: skill_name.to_owned(),
            description: "Send a signal reply".to_owned(),
            steps: vec!["do the thing".to_owned()],
            tools_used: vec!["signal".to_owned()],
            domain_tags: vec!["communication".to_owned()],
            origin: "seeded".to_owned(),
            triggers: vec![],
            always: false,
        })
        .expect("skill content serializes");

        Fact {
            id: mneme::id::FactId::new("skill-alice-signal").expect("valid fact id"),
            nous_id: "alice".to_owned(),
            fact_type: "skill".to_owned(),
            content,
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: jiff::Timestamp::from_second(1_700_000_000).expect("valid timestamp"),
                valid_to: far_future(),
                recorded_at: jiff::Timestamp::from_second(1_700_000_100).expect("valid timestamp"),
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: 24.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[cfg(feature = "recall")]
    async fn skills_command_uses_seeded_knowledge_store() {
        let (_dir, oikos) = make_oikos();
        let mut knowledge_stores = HashMap::new();
        let store = mneme::knowledge_store::KnowledgeStore::open_mem()
            .expect("open in-memory knowledge store");
        let skill_fact = make_skill_fact("signal-send");
        store.insert_fact(&skill_fact).expect("insert skill fact");
        knowledge_stores.insert("shared".to_owned(), store);

        let mut mgr = make_skill_manager(oikos, knowledge_stores);
        let _handle = mgr
            .spawn(make_config(), PipelineConfig::default())
            .await
            .expect("spawn alice");

        let reply = execute_command(
            &command::Command::Skills,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
        )
        .await;

        assert!(reply.contains("signal-send"), "{reply}");
        assert!(!reply.contains("No skills available"), "{reply}");

        mgr.shutdown_all().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blackboard_command_uses_session_adapter() {
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let tool_services = make_tool_services(&session_store);
        let mgr = make_dispatch_manager(oikos, Some(tool_services));

        let blackboard_store = mgr.blackboard_store().expect("blackboard store");
        blackboard_store
            .write("goal", "finish the demo", "alice", 3600)
            .expect("write blackboard entry");

        let reply = execute_command(
            &command::Command::Blackboard,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
        )
        .await;

        assert!(
            reply.contains("[goal] = finish the demo (by alice)"),
            "{reply}"
        );
        assert!(!reply.contains("Blackboard empty"), "{reply}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_state_falls_back_without_stores() {
        let (_dir, oikos) = make_oikos();
        let mgr = make_dispatch_manager(oikos, None);

        let skills_reply = execute_command(
            &command::Command::Skills,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
        )
        .await;
        assert!(
            skills_reply.contains("No skills available"),
            "{skills_reply}"
        );

        let blackboard_reply = execute_command(
            &command::Command::Blackboard,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
        )
        .await;
        assert!(
            blackboard_reply.contains("Blackboard empty"),
            "{blackboard_reply}"
        );
    }
}
