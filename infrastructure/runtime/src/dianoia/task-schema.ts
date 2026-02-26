// SQLite DDL for the dianoia task system
// Tasks are scoped to planning projects/phases but can also exist standalone (DAILY)

export const TASK_V1_DDL = `
CREATE TABLE IF NOT EXISTS planning_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES planning_projects(id) ON DELETE SET NULL,
  phase_id TEXT REFERENCES planning_phases(id) ON DELETE SET NULL,
  parent_id TEXT REFERENCES planning_tasks(id) ON DELETE CASCADE,

  -- Human-readable sequential ID: PROJ-001, DAILY-001, etc.
  task_id TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'active', 'done', 'failed', 'skipped', 'blocked')),
  priority TEXT NOT NULL DEFAULT 'medium' CHECK(priority IN ('critical', 'high', 'medium', 'low')),

  -- Execution context (ENG-03: enriched task schema)
  action TEXT,            -- what to do (imperative instruction)
  verify TEXT,            -- how to verify it worked
  files TEXT NOT NULL DEFAULT '[]',  -- JSON array of relevant file paths
  must_haves TEXT NOT NULL DEFAULT '[]', -- JSON array of acceptance criteria
  context_budget INTEGER, -- max tokens for sub-agent context

  -- Dependencies
  blocked_by TEXT NOT NULL DEFAULT '[]',  -- JSON array of task_ids this is blocked by
  blocks TEXT NOT NULL DEFAULT '[]',      -- JSON array of task_ids this blocks

  -- Hierarchy (max 4 levels enforced in code)
  depth INTEGER NOT NULL DEFAULT 0,

  -- Metadata
  assignee TEXT,          -- nous ID or null
  tags TEXT NOT NULL DEFAULT '[]', -- JSON array of strings
  completed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_tasks_project ON planning_tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_phase ON planning_tasks(phase_id);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON planning_tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON planning_tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_task_id ON planning_tasks(task_id);
`;
