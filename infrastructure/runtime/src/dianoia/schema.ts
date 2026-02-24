// SQLite DDL for the dianoia (planning) module — all 5 planning tables
export const PLANNING_V20_DDL = `
CREATE TABLE IF NOT EXISTS planning_projects (
  id TEXT PRIMARY KEY,
  nous_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  goal TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'idle' CHECK(state IN ('idle', 'questioning', 'researching', 'requirements', 'roadmap', 'phase-planning', 'executing', 'verifying', 'complete', 'blocked', 'abandoned')),
  config TEXT NOT NULL DEFAULT '{}',
  context_hash TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_projects_nous ON planning_projects(nous_id);

CREATE TABLE IF NOT EXISTS planning_phases (
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

CREATE INDEX IF NOT EXISTS idx_planning_phases_project ON planning_phases(project_id, phase_order);

CREATE TABLE IF NOT EXISTS planning_requirements (
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

CREATE INDEX IF NOT EXISTS idx_planning_requirements_project ON planning_requirements(project_id);

CREATE TABLE IF NOT EXISTS planning_checkpoints (
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

CREATE INDEX IF NOT EXISTS idx_planning_research_project ON planning_research(project_id);
`;

export const PLANNING_V20_MIGRATION_ENTRY = PLANNING_V20_DDL;

export const PLANNING_V21_MIGRATION = `ALTER TABLE planning_projects ADD COLUMN project_context TEXT`;

export const PLANNING_V22_MIGRATION = `ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete' CHECK(status IN ('complete', 'partial', 'failed'))`;

export const PLANNING_V23_MIGRATION = `ALTER TABLE planning_requirements ADD COLUMN rationale TEXT`;

export const PLANNING_V24_MIGRATION = `ALTER TABLE planning_phases ADD COLUMN verification_result TEXT`;

export const PLANNING_V25_MIGRATION = `
CREATE TABLE IF NOT EXISTS planning_spawn_records (
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
`;
