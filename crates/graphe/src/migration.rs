//! Versioned schema migration runner.
#![cfg_attr(
    test,
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use snafu::ResultExt;
use tracing::{info, warn};

use crate::error::{self, Result};
use crate::schema::DDL;

/// A single versioned migration.
pub(crate) struct Migration {
    /// Monotonically increasing version number.
    pub version: u32,
    /// Human-readable summary of what this migration does.
    pub description: &'static str,
    /// SQL to apply the migration.
    pub up: &'static str,
    /// SQL to reverse the migration.
    #[expect(
        dead_code,
        reason = "down migrations preserved for future rollback support"
    )]
    pub down: &'static str,
}

/// All registered migrations, in version order.
///
/// WHY versions 1-31 are reconstructed: The deployed database (sessions.db)
/// reached schema version 31 via a release binary whose migration source was
/// never committed. The schema_version rows have empty checksums, so
/// verification is skipped for deployed databases. These migrations
/// reconstruct the incremental path so that:
///   1. A fresh database reaches the same schema as the deployed one.
///   2. The "schema too new" guard no longer blocks startup.
pub(crate) static MIGRATIONS: &[Migration] = &[
    // ── v1: base schema ─────────────────────────────────────────────────
    Migration {
        version: 1,
        description: "base schema — sessions, messages, usage, distillations, agent_notes",
        up: DDL,
        down: "DROP TABLE IF EXISTS agent_notes;
DROP TABLE IF EXISTS distillations;
DROP TABLE IF EXISTS usage;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS sessions;",
    },
    // ── v2: blackboard ──────────────────────────────────────────────────
    Migration {
        version: 2,
        description: "blackboard — shared agent state with TTL",
        up: "CREATE TABLE IF NOT EXISTS blackboard (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    author_nous_id TEXT NOT NULL,
    ttl_seconds INTEGER DEFAULT 3600,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_blackboard_key ON blackboard(key);
CREATE INDEX IF NOT EXISTS idx_blackboard_expires ON blackboard(expires_at);",
        down: "DROP TABLE IF EXISTS blackboard;",
    },
    // ── v3-v9: session columns ──────────────────────────────────────────
    Migration {
        version: 3,
        description: "sessions — token tracking and bootstrap hash",
        up: "ALTER TABLE sessions ADD COLUMN last_input_tokens INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN bootstrap_hash TEXT;
ALTER TABLE sessions ADD COLUMN distillation_count INTEGER DEFAULT 0;",
        down: "ALTER TABLE sessions DROP COLUMN last_input_tokens;
ALTER TABLE sessions DROP COLUMN bootstrap_hash;
ALTER TABLE sessions DROP COLUMN distillation_count;",
    },
    Migration {
        version: 4,
        description: "sessions — thinking mode configuration",
        up: "ALTER TABLE sessions ADD COLUMN thinking_enabled INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN thinking_budget INTEGER DEFAULT 10000;",
        down: "ALTER TABLE sessions DROP COLUMN thinking_enabled;
ALTER TABLE sessions DROP COLUMN thinking_budget;",
    },
    Migration {
        version: 5,
        description: "sessions — thread binding and transport",
        up: "ALTER TABLE sessions ADD COLUMN thread_id TEXT;
ALTER TABLE sessions ADD COLUMN transport TEXT;
ALTER TABLE sessions ADD COLUMN working_state TEXT;",
        down: "ALTER TABLE sessions DROP COLUMN thread_id;
ALTER TABLE sessions DROP COLUMN transport;
ALTER TABLE sessions DROP COLUMN working_state;",
    },
    Migration {
        version: 6,
        description: "sessions — session type and distillation tracking",
        up: "ALTER TABLE sessions ADD COLUMN session_type TEXT DEFAULT 'primary';
ALTER TABLE sessions ADD COLUMN last_distilled_at TEXT;
ALTER TABLE sessions ADD COLUMN computed_context_tokens INTEGER DEFAULT 0;
ALTER TABLE sessions ADD COLUMN distillation_priming TEXT;",
        down: "ALTER TABLE sessions DROP COLUMN session_type;
ALTER TABLE sessions DROP COLUMN last_distilled_at;
ALTER TABLE sessions DROP COLUMN computed_context_tokens;
ALTER TABLE sessions DROP COLUMN distillation_priming;",
    },
    // ── v7-v8: session indexes ──────────────────────────────────────────
    Migration {
        version: 7,
        description: "sessions — session_type and thread indexes",
        up: "CREATE INDEX IF NOT EXISTS idx_sessions_type ON sessions(session_type);
CREATE INDEX IF NOT EXISTS idx_sessions_thread ON sessions(thread_id);",
        down: "DROP INDEX IF EXISTS idx_sessions_type;
DROP INDEX IF EXISTS idx_sessions_thread;",
    },
    // ── v8-v10: threads, transport, thread summaries ────────────────────
    Migration {
        version: 8,
        description: "threads — conversation thread identity tracking",
        up: "CREATE TABLE IF NOT EXISTS threads (
    id TEXT PRIMARY KEY,
    nous_id TEXT NOT NULL,
    identity TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(nous_id, identity)
);
CREATE INDEX IF NOT EXISTS idx_threads_nous ON threads(nous_id);",
        down: "DROP TABLE IF EXISTS threads;",
    },
    Migration {
        version: 9,
        description: "transport_bindings — channel-to-thread mapping",
        up: "CREATE TABLE IF NOT EXISTS transport_bindings (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    transport TEXT NOT NULL,
    channel_key TEXT NOT NULL,
    last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(transport, channel_key)
);
CREATE INDEX IF NOT EXISTS idx_bindings_thread ON transport_bindings(thread_id);",
        down: "DROP TABLE IF EXISTS transport_bindings;",
    },
    Migration {
        version: 10,
        description: "thread_summaries — cached thread context",
        up: "CREATE TABLE IF NOT EXISTS thread_summaries (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id),
    summary TEXT NOT NULL DEFAULT '',
    key_facts TEXT NOT NULL DEFAULT '[]',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);",
        down: "DROP TABLE IF EXISTS thread_summaries;",
    },
    // ── v11-v12: auth and contacts ──────────────────────────────────────
    Migration {
        version: 11,
        description: "auth_sessions — JWT refresh token tracking",
        up: "CREATE TABLE IF NOT EXISTS auth_sessions (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    role TEXT NOT NULL,
    refresh_token_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_used_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0,
    ip_address TEXT,
    user_agent TEXT
);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_username ON auth_sessions(username);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_expires ON auth_sessions(expires_at);",
        down: "DROP TABLE IF EXISTS auth_sessions;",
    },
    Migration {
        version: 12,
        description: "contact_requests and approved_contacts — contact approval workflow",
        up: "CREATE TABLE IF NOT EXISTS contact_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL,
    sender_name TEXT,
    channel TEXT NOT NULL DEFAULT 'signal',
    account_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'approved', 'denied', 'expired')),
    challenge_code TEXT,
    approved_by TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    resolved_at TEXT,
    UNIQUE(sender, channel, account_id)
);
CREATE INDEX IF NOT EXISTS idx_contacts_status ON contact_requests(status);

CREATE TABLE IF NOT EXISTS approved_contacts (
    sender TEXT NOT NULL,
    channel TEXT NOT NULL DEFAULT 'signal',
    account_id TEXT,
    approved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    approved_by TEXT,
    UNIQUE(sender, channel, account_id)
);
CREATE INDEX IF NOT EXISTS idx_approved_sender ON approved_contacts(sender, channel);",
        down: "DROP TABLE IF EXISTS approved_contacts;
DROP TABLE IF EXISTS contact_requests;",
    },
    // ── v13-v15: observability tables ────────────────────────────────────
    Migration {
        version: 13,
        description: "tool_stats — per-tool success/failure tracking",
        up: "CREATE TABLE IF NOT EXISTS tool_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    nous_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    success INTEGER NOT NULL DEFAULT 1,
    error_message TEXT,
    duration_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_tool_stats_lookup ON tool_stats(nous_id, tool_name, created_at);",
        down: "DROP TABLE IF EXISTS tool_stats;",
    },
    Migration {
        version: 14,
        description: "interaction_signals — conversation quality signals",
        up: "CREATE TABLE IF NOT EXISTS interaction_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    signal TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_signals_session ON interaction_signals(session_id);
CREATE INDEX IF NOT EXISTS idx_signals_nous ON interaction_signals(nous_id);",
        down: "DROP TABLE IF EXISTS interaction_signals;",
    },
    Migration {
        version: 15,
        description: "sub_agent_log — delegated agent execution tracking",
        up: "CREATE TABLE IF NOT EXISTS sub_agent_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    parent_session_id TEXT NOT NULL,
    parent_nous_id TEXT NOT NULL,
    role TEXT,
    agent_id TEXT NOT NULL,
    task TEXT NOT NULL,
    model TEXT,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    total_cost_tokens INTEGER DEFAULT 0,
    tool_calls INTEGER DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'completed',
    error TEXT,
    duration_ms INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_sub_agent_parent ON sub_agent_log(parent_session_id);",
        down: "DROP TABLE IF EXISTS sub_agent_log;",
    },
    // ── v16-v18: cross-agent messaging ──────────────────────────────────
    Migration {
        version: 16,
        description: "cross_agent_messages — inter-agent communication",
        up: "CREATE TABLE IF NOT EXISTS cross_agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_session_id TEXT NOT NULL,
    target_nous_id TEXT NOT NULL,
    target_session_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('send', 'ask', 'spawn')),
    content TEXT NOT NULL,
    response TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'delivered', 'responded', 'timeout', 'error')),
    timeout_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    responded_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_cam_target ON cross_agent_messages(target_nous_id, status);",
        down: "DROP TABLE IF EXISTS cross_agent_messages;",
    },
    Migration {
        version: 17,
        description: "routing_cache — channel-to-nous routing",
        up: "CREATE TABLE IF NOT EXISTS routing_cache (
    channel TEXT NOT NULL,
    peer_kind TEXT,
    peer_id TEXT,
    account_id TEXT,
    nous_id TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    UNIQUE(channel, peer_kind, peer_id, account_id)
);",
        down: "DROP TABLE IF EXISTS routing_cache;",
    },
    Migration {
        version: 18,
        description: "message_queue — inbound message buffer for sessions",
        up: "CREATE TABLE IF NOT EXISTS message_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    content TEXT NOT NULL,
    sender TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_queue_session ON message_queue(session_id);",
        down: "DROP TABLE IF EXISTS message_queue;",
    },
    // ── v19-v21: distillation and reflection ────────────────────────────
    Migration {
        version: 19,
        description: "distillation_locks — prevent concurrent distillation",
        up: "CREATE TABLE IF NOT EXISTS distillation_locks (
    session_id TEXT PRIMARY KEY,
    nous_id TEXT NOT NULL,
    locked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);",
        down: "DROP TABLE IF EXISTS distillation_locks;",
    },
    Migration {
        version: 20,
        description: "distillation_log — detailed distillation history",
        up: "CREATE TABLE IF NOT EXISTS distillation_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    distilled_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    messages_before INTEGER NOT NULL,
    messages_after INTEGER NOT NULL,
    tokens_before INTEGER NOT NULL,
    tokens_after INTEGER NOT NULL,
    facts_extracted INTEGER DEFAULT 0,
    decisions_extracted INTEGER DEFAULT 0,
    open_items_extracted INTEGER DEFAULT 0,
    flush_succeeded INTEGER DEFAULT 1,
    errors TEXT,
    distillation_number INTEGER DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_distill_log_session ON distillation_log(session_id);",
        down: "DROP TABLE IF EXISTS distillation_log;",
    },
    Migration {
        version: 21,
        description: "reflection_log — periodic self-reflection results",
        up: "CREATE TABLE IF NOT EXISTS reflection_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    nous_id TEXT NOT NULL,
    reflected_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    sessions_reviewed INTEGER NOT NULL DEFAULT 0,
    messages_reviewed INTEGER NOT NULL DEFAULT 0,
    patterns_found INTEGER NOT NULL DEFAULT 0,
    contradictions_found INTEGER NOT NULL DEFAULT 0,
    corrections_found INTEGER NOT NULL DEFAULT 0,
    preferences_found INTEGER NOT NULL DEFAULT 0,
    relationships_found INTEGER NOT NULL DEFAULT 0,
    unresolved_threads_found INTEGER NOT NULL DEFAULT 0,
    memories_stored INTEGER NOT NULL DEFAULT 0,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    findings TEXT NOT NULL DEFAULT '{}',
    errors TEXT
);
CREATE INDEX IF NOT EXISTS idx_reflection_nous ON reflection_log(nous_id);
CREATE INDEX IF NOT EXISTS idx_reflection_date ON reflection_log(reflected_at);",
        down: "DROP TABLE IF EXISTS reflection_log;",
    },
    // ── v22: plans ──────────────────────────────────────────────────────
    Migration {
        version: 22,
        description: "plans — cost-gated execution plans",
        up: "CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'awaiting_approval',
    steps TEXT NOT NULL,
    total_estimated_cost_cents REAL NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    resolved_at TEXT,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
CREATE INDEX IF NOT EXISTS idx_plans_session ON plans(session_id, status);",
        down: "DROP TABLE IF EXISTS plans;",
    },
    // ── v23-v25: planning engine core ───────────────────────────────────
    Migration {
        version: 23,
        description: "planning_projects — top-level planning containers",
        up: "CREATE TABLE IF NOT EXISTS planning_projects (
    id TEXT PRIMARY KEY,
    nous_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    goal TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'idle' CHECK(state IN ('idle', 'questioning', 'researching', 'requirements', 'roadmap', 'discussing', 'phase-planning', 'executing', 'verifying', 'complete', 'blocked', 'abandoned')),
    config TEXT NOT NULL DEFAULT '{}',
    context_hash TEXT NOT NULL,
    project_context TEXT,
    project_dir TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_projects_nous ON planning_projects(nous_id);",
        down: "DROP TABLE IF EXISTS planning_projects;",
    },
    Migration {
        version: 24,
        description: "planning_phases — project phase breakdown",
        up: "CREATE TABLE IF NOT EXISTS planning_phases (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    goal TEXT NOT NULL,
    requirements TEXT NOT NULL DEFAULT '[]',
    success_criteria TEXT NOT NULL DEFAULT '[]',
    plan TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'executing', 'complete', 'failed', 'skipped')),
    phase_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_phases_project ON planning_phases(project_id, phase_order);",
        down: "DROP TABLE IF EXISTS planning_phases;",
    },
    Migration {
        version: 25,
        description: "planning_requirements — tiered requirement tracking",
        up: "CREATE TABLE IF NOT EXISTS planning_requirements (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT,
    req_id TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'v1' CHECK(tier IN ('v1', 'v2', 'out-of-scope')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'validated', 'skipped')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_requirements_project ON planning_requirements(project_id);",
        down: "DROP TABLE IF EXISTS planning_requirements;",
    },
    // ── v26-v28: planning support tables ────────────────────────────────
    Migration {
        version: 26,
        description: "planning_checkpoints and planning_research",
        up: "CREATE TABLE IF NOT EXISTS planning_checkpoints (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    question TEXT NOT NULL,
    decision TEXT,
    context TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_checkpoints_project ON planning_checkpoints(project_id);

CREATE TABLE IF NOT EXISTS planning_research (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase TEXT NOT NULL,
    dimension TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_research_project ON planning_research(project_id);",
        down: "DROP TABLE IF EXISTS planning_research;
DROP TABLE IF EXISTS planning_checkpoints;",
    },
    Migration {
        version: 27,
        description: "planning_messages and planning_discussions",
        up: "CREATE TABLE IF NOT EXISTS planning_messages (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT,
    source TEXT NOT NULL CHECK(source IN ('user', 'agent', 'sub-agent', 'system')),
    source_session_id TEXT,
    content TEXT NOT NULL,
    priority TEXT NOT NULL DEFAULT 'normal' CHECK(priority IN ('low', 'normal', 'high', 'critical')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'delivered', 'expired')),
    delivered_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_messages_project ON planning_messages(project_id, status);
CREATE INDEX IF NOT EXISTS idx_planning_messages_phase ON planning_messages(phase_id, status);

CREATE TABLE IF NOT EXISTS planning_discussions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT NOT NULL,
    question TEXT NOT NULL,
    options TEXT NOT NULL DEFAULT '[]',
    recommendation TEXT,
    decision TEXT,
    user_note TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'answered', 'skipped')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_discussions_project ON planning_discussions(project_id);
CREATE INDEX IF NOT EXISTS idx_planning_discussions_phase ON planning_discussions(phase_id);",
        down: "DROP TABLE IF EXISTS planning_discussions;
DROP TABLE IF EXISTS planning_messages;",
    },
    Migration {
        version: 28,
        description: "planning_decisions — decision audit trail",
        up: "CREATE TABLE IF NOT EXISTS planning_decisions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT,
    source TEXT NOT NULL CHECK(source IN ('user', 'agent', 'checkpoint', 'system')),
    type TEXT NOT NULL,
    summary TEXT NOT NULL,
    rationale TEXT,
    context TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_decisions_project ON planning_decisions(project_id);
CREATE INDEX IF NOT EXISTS idx_planning_decisions_phase ON planning_decisions(phase_id);",
        down: "DROP TABLE IF EXISTS planning_decisions;",
    },
    // ── v29: planning annotations and edit history ──────────────────────
    Migration {
        version: 29,
        description: "planning_annotations and planning_edit_history",
        up: "CREATE TABLE IF NOT EXISTS planning_annotations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    target_type TEXT NOT NULL CHECK(target_type IN ('requirement', 'phase', 'project', 'discussion')),
    target_id TEXT NOT NULL,
    author TEXT NOT NULL,
    content TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_annotations_target ON planning_annotations(project_id, target_type, target_id);

CREATE TABLE IF NOT EXISTS planning_edit_history (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    target_type TEXT NOT NULL CHECK(target_type IN ('requirement', 'phase', 'project', 'discussion', 'checkpoint')),
    target_id TEXT NOT NULL,
    field TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT,
    author TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_edit_history_target ON planning_edit_history(project_id, target_type, target_id);",
        down: "DROP TABLE IF EXISTS planning_edit_history;
DROP TABLE IF EXISTS planning_annotations;",
    },
    // ── v30: planning execution tracking ────────────────────────────────
    Migration {
        version: 30,
        description: "planning_spawn_records and planning_turn_counts",
        up: "CREATE TABLE IF NOT EXISTS planning_spawn_records (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT NOT NULL REFERENCES planning_phases(id) ON DELETE CASCADE,
    agent_session_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running', 'complete', 'failed', 'done', 'skipped', 'zombie')),
    result TEXT,
    wave INTEGER NOT NULL DEFAULT 0,
    started_at TEXT,
    completed_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_planning_spawn_records_project ON planning_spawn_records(project_id);
CREATE INDEX IF NOT EXISTS idx_planning_spawn_records_phase ON planning_spawn_records(phase_id);

CREATE TABLE IF NOT EXISTS planning_turn_counts (
    project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
    phase_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    turn_count INTEGER NOT NULL DEFAULT 0,
    token_count INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (project_id, phase_id, nous_id)
);",
        down: "DROP TABLE IF EXISTS planning_turn_counts;
DROP TABLE IF EXISTS planning_spawn_records;",
    },
    // ── v31: audit, display_name, cross-agent extras, planning extras ───
    Migration {
        version: 31,
        description: "audit_log, sessions display_name, cross-agent extras, planning schema evolution",
        up: "CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    actor TEXT NOT NULL,
    role TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT,
    ip TEXT,
    user_agent TEXT,
    status INTEGER NOT NULL,
    duration_ms INTEGER,
    checksum TEXT,
    previous_checksum TEXT
);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);

ALTER TABLE sessions ADD COLUMN display_name TEXT;

ALTER TABLE cross_agent_messages ADD COLUMN source_nous_id TEXT;
ALTER TABLE cross_agent_messages ADD COLUMN surfaced_in_session TEXT;
ALTER TABLE cross_agent_messages ADD COLUMN content_hash TEXT;
CREATE INDEX IF NOT EXISTS idx_xagent_hash ON cross_agent_messages(content_hash, created_at);

ALTER TABLE planning_phases ADD COLUMN verification_result TEXT;
ALTER TABLE planning_phases ADD COLUMN dependencies TEXT NOT NULL DEFAULT '[]';

ALTER TABLE planning_requirements ADD COLUMN rationale TEXT;
ALTER TABLE planning_requirements ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]';
ALTER TABLE planning_requirements ADD COLUMN blocked_by TEXT NOT NULL DEFAULT '[]';

ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete' CHECK(status IN ('complete', 'partial', 'failed'));",
        down: "DROP INDEX IF EXISTS idx_xagent_hash;
DROP INDEX IF EXISTS idx_audit_timestamp;
DROP INDEX IF EXISTS idx_audit_actor;
DROP TABLE IF EXISTS audit_log;",
    },
    // ── v32: blackboard UNIQUE constraint on key ───────────────────────
    Migration {
        version: 32,
        description: "blackboard — add UNIQUE constraint on key for upsert support",
        up: "CREATE UNIQUE INDEX IF NOT EXISTS idx_blackboard_key_unique ON blackboard(key);
DROP INDEX IF EXISTS idx_blackboard_key;",
        down: "DROP INDEX IF EXISTS idx_blackboard_key_unique;
CREATE INDEX IF NOT EXISTS idx_blackboard_key ON blackboard(key);",
    },
];

/// Outcome of a migration run.
///
/// WHY fields are dead-code-allowed in non-test builds: every field is
/// read by `#[cfg(test)] mod tests` (the migration test suite). The
/// non-test build constructs but doesn't read them — they exist for
/// the test assertions and as a future API surface for diagnostics.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "fields are read by #[cfg(test)] migration test suite")
)]
pub(crate) struct MigrationResult {
    /// Versions applied during this run.
    pub applied: Vec<u32>,
    /// Schema version after migration.
    pub current_version: u32,
    /// True if the database was brand new (no tables existed).
    pub was_fresh: bool,
}

/// Pending migration info for dry-run reporting.
///
/// See [`MigrationResult`] for the dead-code rationale.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "fields are read by #[cfg(test)] migration test suite")
)]
pub(crate) struct PendingMigration {
    /// Version number that would be applied.
    pub version: u32,
    /// Human-readable summary of the migration.
    pub description: &'static str,
}

/// Apply all pending migrations to the database and verify existing checksums.
///
/// Migrations are applied in version order. Each migration runs inside a
/// transaction: the up SQL executes, then the version and its SHA-256 checksum
/// are recorded. If any migration fails, the transaction rolls back and the
/// error is returned.
///
/// Before applying new migrations, checksums of already-applied migrations are
/// verified. A mismatch means the migration SQL was altered after application
/// and returns [`error::Error::ChecksumMismatch`].
///
/// # Errors
///
/// Returns [`error::Error::Database`] if `SQLite` operations fail.
/// Returns [`error::Error::Migration`] if a migration's SQL fails.
/// Returns [`error::Error::ChecksumMismatch`] if a recorded checksum does not
/// match the current migration SQL.
pub(crate) fn run_migrations(conn: &Connection) -> Result<MigrationResult> {
    let was_fresh = !schema_version_table_exists(conn);

    bootstrap_version_table(conn)?;

    let current = get_schema_version(conn);

    // Check if database schema is newer than this binary supports.
    // This prevents running an old binary against a newer schema.
    let max_supported_version = MIGRATIONS.last().map_or(0, |m| m.version);
    if current > max_supported_version {
        return Err(error::SchemaTooNewSnafu {
            current,
            max: max_supported_version,
        }
        .build());
    }

    // Verify checksums for all already-applied migrations before proceeding.
    verify_migration_checksums(conn, current)?;

    let mut applied = Vec::new();

    for migration in MIGRATIONS {
        if migration.version <= current {
            continue;
        }

        info!(
            version = migration.version,
            description = migration.description,
            "applying migration"
        );

        let tx = conn.unchecked_transaction().context(error::DatabaseSnafu)?;

        tx.execute_batch(migration.up)
            .context(error::MigrationSnafu {
                version: migration.version,
            })?;

        tx.execute(
            "INSERT INTO schema_version (version, description, checksum) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                migration.version,
                migration.description,
                compute_checksum(migration.up),
            ],
        )
        .context(error::MigrationSnafu {
            version: migration.version,
        })?;

        // WHY: PRAGMA user_version provides a lightweight, standard SQLite
        // mechanism for external tools to query schema version without
        // knowing about the schema_version table.
        tx.pragma_update(None, "user_version", migration.version)
            .context(error::MigrationSnafu {
                version: migration.version,
            })?;

        tx.commit().context(error::MigrationSnafu {
            version: migration.version,
        })?;

        applied.push(migration.version);
    }

    let current_version = applied.last().copied().unwrap_or(current);

    if !applied.is_empty() {
        info!(
            from = current,
            to = current_version,
            count = applied.len(),
            "migrations applied"
        );
    }

    reconcile_user_version(conn, current_version)?;

    Ok(MigrationResult {
        applied,
        current_version,
        was_fresh,
    })
}

/// Report pending migrations without applying them.
///
/// # Errors
///
/// Returns [`error::Error::Database`] if `SQLite` operations fail.
#[expect(dead_code, reason = "migration check for CLI preflight diagnostics")]
pub(crate) fn check_migrations(conn: &Connection) -> Result<Vec<PendingMigration>> {
    bootstrap_version_table(conn)?;
    let current = get_schema_version(conn);

    Ok(MIGRATIONS
        .iter()
        .filter(|m| m.version > current)
        .map(|m| PendingMigration {
            version: m.version,
            description: m.description,
        })
        .collect())
}

/// Verify that all applied migrations match their recorded checksums.
///
/// Only migrations whose `checksum` column is non-empty are verified; rows
/// without a checksum (legacy databases upgraded before checksum support was
/// added) are skipped.
///
/// # Errors
///
/// Returns [`error::Error::Database`] if a `SQLite` query fails.
/// Returns [`error::Error::ChecksumMismatch`] if a stored checksum does not
/// match the checksum computed from the current migration SQL.
pub(crate) fn verify_migration_checksums(conn: &Connection, current_version: u32) -> Result<()> {
    for migration in MIGRATIONS {
        if migration.version > current_version {
            break;
        }

        let stored: Option<String> = conn
            .query_row(
                "SELECT checksum FROM schema_version WHERE version = ?1",
                rusqlite::params![migration.version],
                |row| row.get(0),
            )
            .optional()
            .context(error::DatabaseSnafu)?;

        if let Some(stored_checksum) = stored {
            // Skip empty checksums: legacy rows recorded before checksum support.
            if stored_checksum.is_empty() {
                continue;
            }

            let expected = compute_checksum(migration.up);
            if stored_checksum != expected {
                return Err(error::ChecksumMismatchSnafu {
                    version: migration.version,
                    expected,
                    found: stored_checksum,
                }
                .build());
            }
        }
    }

    Ok(())
}

/// Ensure the `schema_version` table exists with all expected columns.
fn bootstrap_version_table(conn: &Connection) -> Result<()> {
    if schema_version_table_exists(conn) {
        // NOTE: Older databases may lack the description column.
        if !has_description_column(conn) {
            conn.execute_batch(
                "ALTER TABLE schema_version ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            )
            .context(error::DatabaseSnafu)?;
        }
        // NOTE: Databases predating checksum support lack the checksum column.
        if !has_checksum_column(conn) {
            conn.execute_batch(
                "ALTER TABLE schema_version ADD COLUMN checksum TEXT NOT NULL DEFAULT ''",
            )
            .context(error::DatabaseSnafu)?;
        }
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            description TEXT NOT NULL DEFAULT '',
            checksum TEXT NOT NULL DEFAULT ''
        )",
    )
    .context(error::DatabaseSnafu)?;

    Ok(())
}

fn schema_version_table_exists(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

fn has_description_column(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('schema_version') WHERE name = 'description'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

fn has_checksum_column(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('schema_version') WHERE name = 'checksum'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Get the current schema version, or 0 if no migrations have been applied.
pub(crate) fn get_schema_version(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Ensure `PRAGMA user_version` matches the `schema_version` table.
///
/// The `schema_version` table is the source of truth. If PRAGMA `user_version`
/// has drifted (e.g. manual editing, partial recovery), reconcile by setting
/// the pragma to match the table.
fn reconcile_user_version(conn: &Connection, table_version: u32) -> Result<()> {
    let pragma_version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context(error::DatabaseSnafu)?;

    if pragma_version != table_version {
        warn!(
            pragma_version,
            table_version, "PRAGMA user_version diverged from schema_version table, reconciling"
        );
        conn.pragma_update(None, "user_version", table_version)
            .context(error::DatabaseSnafu)?;
    }

    Ok(())
}

/// Compute the SHA-256 checksum of the given SQL string, returned as a hex string.
fn compute_checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    // WHY: sha2 0.11 output no longer implements LowerHex; format byte-by-byte
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        Connection::open_in_memory().expect("in-memory SQLite connection should always open")
    }

    /// Latest schema version known to the test build.
    ///
    /// WHY: hardcoding `31` (or any value) here breaks every time a new
    /// migration is added. Reading from `MIGRATIONS` keeps the test in
    /// lockstep with the migration list.
    fn latest_version() -> u32 {
        MIGRATIONS
            .last()
            .map(|m| m.version)
            .expect("MIGRATIONS slice is non-empty")
    }

    #[test]
    fn fresh_database_gets_all_migrations() {
        let conn = fresh_conn();
        let result = run_migrations(&conn).expect("migrations should apply to fresh DB");

        assert!(
            result.was_fresh,
            "fresh database should be reported as fresh"
        );
        let expected: Vec<u32> = (1..=latest_version()).collect();
        assert_eq!(
            result.applied, expected,
            "every migration should be applied to a fresh database"
        );
        assert_eq!(
            result.current_version,
            latest_version(),
            "current version should match latest migration after run"
        );
    }

    #[test]
    fn already_migrated_skips_applied() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("first migration run should succeed");

        let result = run_migrations(&conn).expect("migrations should apply to fresh DB");
        assert!(
            !result.was_fresh,
            "second run should not report the database as fresh"
        );
        assert!(
            result.applied.is_empty(),
            "second run should apply no migrations"
        );
        assert_eq!(
            result.current_version,
            latest_version(),
            "version should still match latest after idempotent run"
        );
    }

    #[test]
    fn version_recorded_in_schema_version() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        let (version, description): (u32, String) = conn
            .query_row(
                "SELECT version, description FROM schema_version WHERE version = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("schema_version row for version 1 should exist after migration");
        assert_eq!(version, 1, "version 1 should be recorded");
        assert!(!description.is_empty(), "description should be non-empty");
    }

    #[test]
    fn dry_run_reports_pending_without_applying() {
        let conn = fresh_conn();
        // NOTE: Bootstrap table but don't apply migrations
        bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed");

        let pending = check_migrations(&conn).unwrap_or_default();
        assert_eq!(
            pending.len(),
            MIGRATIONS.len(),
            "every migration should be pending on a fresh database"
        );
        assert_eq!(
            pending[0].version, 1,
            "first pending migration should be version 1"
        );

        let version = get_schema_version(&conn);
        assert_eq!(version, 0, "schema version should remain 0 after dry run");
    }

    #[test]
    fn dry_run_empty_when_current() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        let pending = check_migrations(&conn).unwrap_or_default();
        assert!(
            pending.is_empty(),
            "no migrations should be pending after full migration"
        );
    }

    #[test]
    fn migration_order_enforced() {
        for window in MIGRATIONS.windows(2) {
            assert!(
                window[0].version < window[1].version,
                "migration {} must come before {}",
                window[0].version,
                window[1].version,
            );
        }
    }

    #[test]
    fn tables_exist_after_migration() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        for table in &[
            "sessions",
            "messages",
            "usage",
            "distillations",
            "agent_notes",
            "blackboard",
            "threads",
            "thread_summaries",
            "transport_bindings",
            "auth_sessions",
            "contact_requests",
            "approved_contacts",
            "tool_stats",
            "interaction_signals",
            "sub_agent_log",
            "cross_agent_messages",
            "routing_cache",
            "message_queue",
            "distillation_locks",
            "distillation_log",
            "reflection_log",
            "plans",
            "planning_projects",
            "planning_phases",
            "planning_requirements",
            "planning_checkpoints",
            "planning_research",
            "planning_messages",
            "planning_discussions",
            "planning_decisions",
            "planning_annotations",
            "planning_edit_history",
            "planning_spawn_records",
            "planning_turn_counts",
            "audit_log",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("table existence query should succeed");
            assert!(exists, "table {table} should exist after migration");
        }
    }

    #[test]
    fn run_migrations_fresh_db_schema_version() {
        let conn = fresh_conn();
        let result = run_migrations(&conn).expect("migrations should apply to fresh DB");
        assert_eq!(
            result.current_version,
            latest_version(),
            "current_version should match latest after full migration"
        );
        let version = get_schema_version(&conn);
        assert_eq!(
            version,
            latest_version(),
            "get_schema_version should return latest after full migration"
        );
    }

    #[test]
    fn run_migrations_idempotent() {
        let conn = fresh_conn();
        let first = run_migrations(&conn).expect("first migration run should succeed");
        let second = run_migrations(&conn).expect("second migration run should succeed idempotently");
        assert_eq!(
            first.current_version, second.current_version,
            "version should be the same across idempotent runs"
        );
        assert!(
            second.applied.is_empty(),
            "second run should apply no migrations"
        );
    }

    #[test]
    fn check_migrations_reports_pending() {
        let conn = fresh_conn();
        let pending = check_migrations(&conn).unwrap_or_default();
        assert_eq!(
            pending.len(),
            MIGRATIONS.len(),
            "all migrations should be pending on a fresh database"
        );
        assert_eq!(
            pending[0].version, 1,
            "first pending migration should be version 1"
        );
    }

    #[test]
    fn get_schema_version_fresh_db() {
        let conn = fresh_conn();
        bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed on fresh DB");
        let version = get_schema_version(&conn);
        assert_eq!(version, 0, "schema version should be 0 on a fresh database");
    }

    #[test]
    fn pragma_user_version_tracks_schema_version() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        let pragma_version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("PRAGMA user_version should be readable after migration");
        assert_eq!(
            pragma_version,
            latest_version(),
            "PRAGMA user_version should match latest migration version"
        );
    }

    #[test]
    fn pragma_user_version_zero_before_migration() {
        let conn = fresh_conn();

        let pragma_version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("check_migrations should return all pending on fresh DB");
        assert_eq!(
            pragma_version, 0,
            "PRAGMA user_version should be 0 on a fresh database"
        );
    }

    #[test]
    fn backward_compat_existing_v1_database() {
        let conn = fresh_conn();

        // NOTE: Simulate an older database: schema_version without description column
        conn.execute_batch(
            "CREATE TABLE schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            )",
        )
        .expect("creating legacy schema_version table should succeed");
        conn.execute_batch(DDL).unwrap_or_default();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .expect("inserting v1 row should succeed");

        let result = run_migrations(&conn).expect("migrations should apply to v1 DB");
        assert!(!result.was_fresh, "upgraded database should not be fresh");
        let expected: Vec<u32> = (2..=latest_version()).collect();
        assert_eq!(
            result.applied, expected,
            "migrations 2..=latest should be applied to a v1 database"
        );
        assert_eq!(
            result.current_version,
            latest_version(),
            "current version should match latest after upgrade"
        );

        assert!(
            has_description_column(&conn),
            "description column should be present after upgrade"
        );
        assert!(
            has_checksum_column(&conn),
            "checksum column should be present after upgrade"
        );
    }

    #[test]
    fn checksum_stored_for_new_migrations() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        for migration in MIGRATIONS {
            let stored: String = conn
                .query_row(
                    "SELECT checksum FROM schema_version WHERE version = ?1",
                    rusqlite::params![migration.version],
                    |row| row.get(0),
                )
                .expect("sqlite_master query should succeed for table existence check");
            assert!(
                !stored.is_empty(),
                "checksum for migration v{} should be non-empty",
                migration.version
            );
            let expected = compute_checksum(migration.up);
            assert_eq!(
                stored, expected,
                "stored checksum for v{} should match computed checksum",
                migration.version
            );
        }
    }

    #[test]
    fn verify_checksums_passes_on_intact_db() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        verify_migration_checksums(&conn, get_schema_version(&conn)).unwrap_or_default();
    }

    #[test]
    fn verify_checksums_detects_tampered_checksum() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        // Tamper with the stored checksum for v1.
        conn.execute(
            "UPDATE schema_version SET checksum = 'deadbeef' WHERE version = 1",
            [],
        )
        .expect("tampering with checksum should succeed");

        let err = verify_migration_checksums(&conn, get_schema_version(&conn))
            .expect_err("verification should fail when checksum is tampered");

        let err_str = err.to_string();
        assert!(
            err_str.contains("v1"),
            "error message should identify the offending migration version"
        );
        assert!(
            err_str.contains("deadbeef"),
            "error message should include the recorded (tampered) checksum"
        );
    }

    #[test]
    fn verify_checksums_skips_empty_checksum_legacy_rows() {
        let conn = fresh_conn();
        // Simulate legacy rows: schema_version with empty checksum.
        bootstrap_version_table(&conn).expect("bootstrap should succeed");
        conn.execute_batch(DDL).unwrap_or_default();
        conn.execute(
            "INSERT INTO schema_version (version, description, checksum) VALUES (1, 'base', '')",
            [],
        )
        .expect("inserting legacy row should succeed");

        // Verification should skip the empty-checksum row without error.
        verify_migration_checksums(&conn, 1).unwrap_or_default();
    }

    #[test]
    fn schema_too_new_returns_error() {
        let conn = fresh_conn();

        // Simulate a database with a newer schema version than the binary supports.
        bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed");
        let future_version = MIGRATIONS.last().map_or(1, |m| m.version + 1);
        conn.execute(
            "INSERT INTO schema_version (version, description, checksum) VALUES (?1, 'future migration', '')",
            rusqlite::params![future_version],
        )
        .expect("creating legacy schema_version table should succeed");
        conn.pragma_update(None, "user_version", future_version)
            .expect("PRAGMA user_version should be readable on fresh DB");

        // Running migrations should fail with SchemaTooNew error.
        let err = run_migrations(&conn)
            .expect_err("should fail when database schema is newer than binary supports");
        let err_str = err.to_string();
        assert!(
            err_str.contains("newer than this binary supports"),
            "error message should indicate schema is too new: {err_str}"
        );
        assert!(
            err_str.contains(&future_version.to_string()),
            "error message should include the current version: {err_str}"
        );
    }
}
