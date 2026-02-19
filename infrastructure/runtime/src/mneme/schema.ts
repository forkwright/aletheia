// SQLite DDL — embedded as string constants for migrations
export const SCHEMA_VERSION = 1;

export const DDL = `
-- Sessions table
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  nous_id TEXT NOT NULL,
  session_key TEXT NOT NULL,
  parent_session_id TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'archived', 'distilled')),
  model TEXT,
  token_count_estimate INTEGER DEFAULT 0,
  message_count INTEGER DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(nous_id, session_key)
);

CREATE INDEX IF NOT EXISTS idx_sessions_nous ON sessions(nous_id);
CREATE INDEX IF NOT EXISTS idx_sessions_key ON sessions(nous_id, session_key);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

-- Messages table
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  seq INTEGER NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant', 'tool_result')),
  content TEXT NOT NULL,
  tool_call_id TEXT,
  tool_name TEXT,
  token_estimate INTEGER DEFAULT 0,
  is_distilled INTEGER DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, seq);

-- Usage tracking per turn
CREATE TABLE IF NOT EXISTS usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  turn_seq INTEGER NOT NULL,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  model TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_usage_session ON usage(session_id);

-- Distillation audit trail
CREATE TABLE IF NOT EXISTS distillations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  messages_before INTEGER NOT NULL,
  messages_after INTEGER NOT NULL,
  tokens_before INTEGER NOT NULL,
  tokens_after INTEGER NOT NULL,
  facts_extracted INTEGER DEFAULT 0,
  model TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Cross-agent message tracking
CREATE TABLE IF NOT EXISTS cross_agent_messages (
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

CREATE INDEX IF NOT EXISTS idx_cam_target ON cross_agent_messages(target_nous_id, status);

-- Routing cache (rebuilt from config on startup)
CREATE TABLE IF NOT EXISTS routing_cache (
  channel TEXT NOT NULL,
  peer_kind TEXT,
  peer_id TEXT,
  account_id TEXT,
  nous_id TEXT NOT NULL,
  priority INTEGER DEFAULT 0,
  UNIQUE(channel, peer_kind, peer_id, account_id)
);

-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
`;

// Incremental migrations — each entry upgrades from (version-1) to version.
// Add new migrations here when schema changes. DDL above is always the v1 baseline.
export const MIGRATIONS: Array<{ version: number; sql: string }> = [
  {
    version: 2,
    sql: `
      ALTER TABLE cross_agent_messages ADD COLUMN source_nous_id TEXT;
      ALTER TABLE cross_agent_messages ADD COLUMN surfaced_in_session TEXT;
    `,
  },
  {
    version: 3,
    sql: `
      CREATE TABLE IF NOT EXISTS contact_requests (
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

      CREATE TABLE IF NOT EXISTS approved_contacts (
        sender TEXT NOT NULL,
        channel TEXT NOT NULL DEFAULT 'signal',
        account_id TEXT,
        approved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        approved_by TEXT,
        UNIQUE(sender, channel, account_id)
      );

      CREATE INDEX IF NOT EXISTS idx_contacts_status ON contact_requests(status);
      CREATE INDEX IF NOT EXISTS idx_approved_sender ON approved_contacts(sender, channel);
    `,
  },
  {
    version: 4,
    sql: `
      ALTER TABLE sessions ADD COLUMN last_input_tokens INTEGER DEFAULT 0;
      ALTER TABLE sessions ADD COLUMN bootstrap_hash TEXT;
      ALTER TABLE sessions ADD COLUMN distillation_count INTEGER DEFAULT 0;
    `,
  },
  {
    version: 5,
    sql: `
      CREATE TABLE IF NOT EXISTS interaction_signals (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL,
        nous_id TEXT NOT NULL,
        turn_seq INTEGER NOT NULL,
        signal TEXT NOT NULL,
        confidence REAL NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
      );
      CREATE INDEX IF NOT EXISTS idx_signals_session ON interaction_signals(session_id);
      CREATE INDEX IF NOT EXISTS idx_signals_nous ON interaction_signals(nous_id);
    `,
  },
  {
    version: 6,
    sql: `
      CREATE TABLE IF NOT EXISTS blackboard (
        id TEXT PRIMARY KEY,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        author_nous_id TEXT NOT NULL,
        ttl_seconds INTEGER DEFAULT 3600,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        expires_at TEXT
      );
      CREATE INDEX IF NOT EXISTS idx_blackboard_key ON blackboard(key);
      CREATE INDEX IF NOT EXISTS idx_blackboard_expires ON blackboard(expires_at);
    `,
  },
  {
    version: 7,
    sql: `
      ALTER TABLE sessions ADD COLUMN thinking_enabled INTEGER DEFAULT 0;
      ALTER TABLE sessions ADD COLUMN thinking_budget INTEGER DEFAULT 10000;
    `,
  },
  {
    version: 8,
    sql: `
      -- Threads: one per (identity, nousId) pair — the persistent relationship
      CREATE TABLE IF NOT EXISTS threads (
        id TEXT PRIMARY KEY,
        nous_id TEXT NOT NULL,
        identity TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        UNIQUE(nous_id, identity)
      );
      CREATE INDEX IF NOT EXISTS idx_threads_nous ON threads(nous_id);

      -- Transport bindings: per-channel connection to a thread (separate lock per transport)
      CREATE TABLE IF NOT EXISTS transport_bindings (
        id TEXT PRIMARY KEY,
        thread_id TEXT NOT NULL REFERENCES threads(id),
        transport TEXT NOT NULL,
        channel_key TEXT NOT NULL,
        last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        UNIQUE(transport, channel_key)
      );
      CREATE INDEX IF NOT EXISTS idx_bindings_thread ON transport_bindings(thread_id);

      -- Thread summaries: running relationship digest (populated by Phase 3 distillation)
      CREATE TABLE IF NOT EXISTS thread_summaries (
        thread_id TEXT PRIMARY KEY REFERENCES threads(id),
        summary TEXT NOT NULL DEFAULT '',
        key_facts TEXT NOT NULL DEFAULT '[]',
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
      );

      -- Link sessions (segments) to threads
      ALTER TABLE sessions ADD COLUMN thread_id TEXT;
      ALTER TABLE sessions ADD COLUMN transport TEXT;
      CREATE INDEX IF NOT EXISTS idx_sessions_thread ON sessions(thread_id);
    `,
  },
];
