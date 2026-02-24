// PlanningStore — SQLite-backed CRUD for all 5 planning tables
import { createHash } from "node:crypto";
import type Database from "better-sqlite3";
import { generateId } from "../koina/crypto.js";
import { PlanningError } from "../koina/errors.js";
import { createLogger } from "../koina/logger.js";
import type {
  DianoiaState,
  PlanningCheckpoint,
  PlanningConfig,
  PlanningPhase,
  PlanningProject,
  PlanningRequirement,
  PlanningResearch,
  ProjectContext,
  SpawnRecord,
  VerificationResult,
} from "./types.js";

const log = createLogger("dianoia");

// NOTE: PlanningStore receives a pre-initialized db instance (from SessionStore in production).
// Wiring is done in Phase 2 (DianoiaOrchestrator). For now, db must already have migration v20 applied.

export class PlanningStore {
  constructor(private db: Database.Database) {}

  // --- Projects ---

  createProject(opts: {
    nousId: string;
    sessionId: string;
    goal: string;
    config: PlanningConfig;
  }): PlanningProject {
    const id = generateId("proj");
    const createdAt = new Date().toISOString();
    const contextHash = createHash("sha256")
      .update(`${opts.goal}|${opts.nousId}|${createdAt}`)
      .digest("hex")
      .slice(0, 16);

    const insert = this.db.transaction(() => {
      this.db
        .prepare(
          `INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash, created_at, updated_at)
           VALUES (?, ?, ?, ?, 'idle', ?, ?, ?, ?)`,
        )
        .run(
          id,
          opts.nousId,
          opts.sessionId,
          opts.goal,
          JSON.stringify(opts.config),
          contextHash,
          createdAt,
          createdAt,
        );
    });

    insert();
    log.debug("createProject", { id, nousId: opts.nousId });

    return this.getProjectOrThrow(id);
  }

  getProject(id: string): PlanningProject | undefined {
    const row = this.db
      .prepare("SELECT * FROM planning_projects WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapProject(row) : undefined;
  }

  getProjectOrThrow(id: string): PlanningProject {
    const project = this.getProject(id);
    if (!project) {
      throw new PlanningError(`Planning project not found: ${id}`, {
        code: "PLANNING_PROJECT_NOT_FOUND",
        context: { id },
      });
    }
    return project;
  }

  listProjects(nousId?: string): PlanningProject[] {
    const rows = nousId
      ? (this.db
          .prepare("SELECT * FROM planning_projects WHERE nous_id = ? ORDER BY created_at DESC")
          .all(nousId) as Array<Record<string, unknown>>)
      : (this.db
          .prepare("SELECT * FROM planning_projects ORDER BY created_at DESC")
          .all() as Array<Record<string, unknown>>);
    return rows.map((r) => this.mapProject(r));
  }

  updateProjectState(id: string, state: DianoiaState): void {
    const update = this.db.transaction(() => {
      const result = this.db
        .prepare(
          `UPDATE planning_projects SET state = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(state, id);
      if (result.changes === 0) {
        throw new PlanningError(`Planning project not found: ${id}`, {
          code: "PLANNING_PROJECT_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
  }

  updateProjectConfig(id: string, config: PlanningConfig): void {
    const update = this.db.transaction(() => {
      const result = this.db
        .prepare(
          `UPDATE planning_projects SET config = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(JSON.stringify(config), id);
      if (result.changes === 0) {
        throw new PlanningError(`Planning project not found: ${id}`, {
          code: "PLANNING_PROJECT_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
  }

  updateProjectGoal(id: string, goal: string): void {
    const update = this.db.transaction(() => {
      const result = this.db
        .prepare(
          `UPDATE planning_projects SET goal = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(goal, id);
      if (result.changes === 0) {
        throw new PlanningError(`Planning project not found: ${id}`, {
          code: "PLANNING_PROJECT_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
  }

  updateProjectContext(id: string, context: ProjectContext): void {
    const update = this.db.transaction(() => {
      const result = this.db
        .prepare(
          `UPDATE planning_projects SET project_context = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(JSON.stringify(context), id);
      if (result.changes === 0) {
        throw new PlanningError(`Planning project not found: ${id}`, {
          code: "PLANNING_PROJECT_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
  }

  deleteProject(id: string): void {
    this.db.prepare("DELETE FROM planning_projects WHERE id = ?").run(id);
  }

  // --- Phases ---

  createPhase(opts: {
    projectId: string;
    name: string;
    goal: string;
    requirements: string[];
    successCriteria: string[];
    phaseOrder: number;
  }): PlanningPhase {
    const id = generateId("phase");

    const insert = this.db.transaction(() => {
      this.db
        .prepare(
          `INSERT INTO planning_phases (id, project_id, name, goal, requirements, success_criteria, phase_order)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          id,
          opts.projectId,
          opts.name,
          opts.goal,
          JSON.stringify(opts.requirements),
          JSON.stringify(opts.successCriteria),
          opts.phaseOrder,
        );
    });

    insert();
    return this.getPhaseOrThrow(id);
  }

  getPhase(id: string): PlanningPhase | undefined {
    const row = this.db
      .prepare("SELECT * FROM planning_phases WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapPhase(row) : undefined;
  }

  getPhaseOrThrow(id: string): PlanningPhase {
    const phase = this.getPhase(id);
    if (!phase) {
      throw new PlanningError(`Planning phase not found: ${id}`, {
        code: "PLANNING_PHASE_NOT_FOUND",
        context: { id },
      });
    }
    return phase;
  }

  listPhases(projectId: string): PlanningPhase[] {
    const rows = this.db
      .prepare("SELECT * FROM planning_phases WHERE project_id = ? ORDER BY phase_order ASC")
      .all(projectId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapPhase(r));
  }

  updatePhaseStatus(
    id: string,
    status: "pending" | "executing" | "complete" | "failed" | "skipped",
  ): void {
    const update = this.db.transaction(() => {
      this.db
        .prepare(
          `UPDATE planning_phases SET status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(status, id);
    });
    update();
  }

  updatePhasePlan(id: string, plan: unknown): void {
    const update = this.db.transaction(() => {
      this.db
        .prepare(
          `UPDATE planning_phases SET plan = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(JSON.stringify(plan), id);
    });
    update();
  }

  // --- Requirements ---

  createRequirement(opts: {
    projectId: string;
    phaseId?: string | null;
    reqId: string;
    description: string;
    category: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale?: string | null;
  }): PlanningRequirement {
    const id = generateId("req");

    const insert = this.db.transaction(() => {
      this.db
        .prepare(
          `INSERT INTO planning_requirements (id, project_id, phase_id, req_id, description, category, tier, rationale)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          id,
          opts.projectId,
          opts.phaseId ?? null,
          opts.reqId,
          opts.description,
          opts.category,
          opts.tier,
          opts.rationale ?? null,
        );
    });

    insert();

    const row = this.db
      .prepare("SELECT * FROM planning_requirements WHERE id = ?")
      .get(id) as Record<string, unknown>;
    return this.mapRequirement(row);
  }

  listRequirements(projectId: string): PlanningRequirement[] {
    const rows = this.db
      .prepare("SELECT * FROM planning_requirements WHERE project_id = ? ORDER BY created_at ASC")
      .all(projectId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapRequirement(r));
  }

  updateRequirement(
    id: string,
    updates: { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null },
  ): void {
    const update = this.db.transaction(() => {
      const sets: string[] = [];
      const vals: unknown[] = [];
      if (updates.tier !== undefined) { sets.push("tier = ?"); vals.push(updates.tier); }
      if (updates.rationale !== undefined) { sets.push("rationale = ?"); vals.push(updates.rationale); }
      if (sets.length === 0) return;
      sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
      vals.push(id);
      const result = this.db
        .prepare(`UPDATE planning_requirements SET ${sets.join(", ")} WHERE id = ?`)
        .run(...vals);
      if (result.changes === 0) {
        throw new PlanningError(`Planning requirement not found: ${id}`, {
          code: "PLANNING_REQUIREMENT_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
  }

  // --- Checkpoints ---

  createCheckpoint(opts: {
    projectId: string;
    type: string;
    question: string;
    context: Record<string, unknown>;
  }): PlanningCheckpoint {
    const id = generateId("ckpt");

    this.db
      .prepare(
        `INSERT INTO planning_checkpoints (id, project_id, type, question, context)
         VALUES (?, ?, ?, ?, ?)`,
      )
      .run(id, opts.projectId, opts.type, opts.question, JSON.stringify(opts.context));

    const row = this.db
      .prepare("SELECT * FROM planning_checkpoints WHERE id = ?")
      .get(id) as Record<string, unknown>;
    return this.mapCheckpoint(row);
  }

  resolveCheckpoint(id: string, decision: string, _meta?: Record<string, unknown>): void {
    const update = this.db.transaction(() => {
      this.db
        .prepare("UPDATE planning_checkpoints SET decision = ? WHERE id = ?")
        .run(decision, id);
    });
    update();
  }

  listCheckpoints(projectId: string): PlanningCheckpoint[] {
    const rows = this.db
      .prepare("SELECT * FROM planning_checkpoints WHERE project_id = ? ORDER BY created_at ASC")
      .all(projectId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapCheckpoint(r));
  }

  // --- Research ---

  createResearch(opts: {
    projectId: string;
    phase: string;
    dimension: string;
    content: string;
    status?: "complete" | "partial" | "failed";
  }): PlanningResearch {
    const id = generateId("res");
    const status = opts.status ?? "complete";

    this.db
      .prepare(
        `INSERT INTO planning_research (id, project_id, phase, dimension, content, status)
         VALUES (?, ?, ?, ?, ?, ?)`,
      )
      .run(id, opts.projectId, opts.phase, opts.dimension, opts.content, status);

    const row = this.db
      .prepare("SELECT * FROM planning_research WHERE id = ?")
      .get(id) as Record<string, unknown>;
    return this.mapResearch(row);
  }

  listResearch(projectId: string): PlanningResearch[] {
    const rows = this.db
      .prepare("SELECT * FROM planning_research WHERE project_id = ? ORDER BY created_at ASC")
      .all(projectId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapResearch(r));
  }

  // --- Spawn Records (Phase 7+) ---

  createSpawnRecord(opts: {
    projectId: string;
    phaseId: string;
    agentSessionId?: string;
    wave?: number;
    waveNumber?: number;
  }): SpawnRecord {
    const id = generateId("spawn");
    const now = new Date().toISOString();
    const waveNum = opts.waveNumber ?? opts.wave ?? 0;
    const agentSessionId = opts.agentSessionId ?? "";

    this.db
      .prepare(
        `INSERT INTO planning_spawn_records (id, project_id, phase_id, agent_session_id, status, wave, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'pending', ?, ?, ?)`,
      )
      .run(id, opts.projectId, opts.phaseId, agentSessionId, waveNum, now, now);

    return {
      id,
      projectId: opts.projectId,
      phaseId: opts.phaseId,
      agentSessionId,
      status: "pending",
      result: null,
      wave: waveNum,
      waveNumber: waveNum,
      startedAt: null,
      completedAt: null,
      errorMessage: null,
      createdAt: now,
      updatedAt: now,
    };
  }

  updateSpawnRecord(id: string, updates: { status?: SpawnRecord["status"]; result?: string; startedAt?: string; completedAt?: string; errorMessage?: string }): void {
    const sets: string[] = [];
    const vals: unknown[] = [];
    if (updates.status !== undefined) { sets.push("status = ?"); vals.push(updates.status); }
    if (updates.result !== undefined) { sets.push("result = ?"); vals.push(updates.result); }
    if (updates.startedAt !== undefined) { sets.push("started_at = ?"); vals.push(updates.startedAt); }
    if (updates.completedAt !== undefined) { sets.push("completed_at = ?"); vals.push(updates.completedAt); }
    if (updates.errorMessage !== undefined) { sets.push("error_message = ?"); vals.push(updates.errorMessage); }
    if (sets.length === 0) return;
    sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    vals.push(id);
    this.db.prepare(`UPDATE planning_spawn_records SET ${sets.join(", ")} WHERE id = ?`).run(...vals);
  }

  listSpawnRecords(projectId: string): SpawnRecord[] {
    const rows = this.db
      .prepare("SELECT * FROM planning_spawn_records WHERE project_id = ? ORDER BY wave ASC, created_at ASC")
      .all(projectId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapSpawnRecord(r));
  }

  getSpawnRecord(id: string): SpawnRecord | undefined {
    const row = this.db.prepare("SELECT * FROM planning_spawn_records WHERE id = ?").get(id) as Record<string, unknown> | undefined;
    return row ? this.mapSpawnRecord(row) : undefined;
  }

  getSpawnRecordOrThrow(id: string): SpawnRecord {
    const record = this.getSpawnRecord(id);
    if (!record) {
      throw new PlanningError(`Spawn record not found: ${id}`, {
        code: "PLANNING_SPAWN_NOT_FOUND",
      });
    }
    return record;
  }

  // --- Phase Verification ---

  updatePhaseVerificationResult(id: string, result: VerificationResult): void {
    this.db
      .prepare(
        `UPDATE planning_phases SET verification_result = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(JSON.stringify(result), id);
  }

  // --- Private mappers ---

  private mapProject(row: Record<string, unknown>): PlanningProject {
    let config: PlanningConfig;
    try {
      config = JSON.parse(row["config"] as string) as PlanningConfig;
    } catch (cause) {
      throw new PlanningError("Corrupt config JSON in planning_projects", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause,
      });
    }
    let projectContext: ProjectContext | null = null;
    if (row["project_context"]) {
      try {
        projectContext = JSON.parse(row["project_context"] as string) as ProjectContext;
      } catch {
        projectContext = null;
      }
    }
    return {
      id: row["id"] as string,
      nousId: row["nous_id"] as string,
      sessionId: row["session_id"] as string,
      goal: row["goal"] as string,
      state: row["state"] as DianoiaState,
      config,
      contextHash: row["context_hash"] as string,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
      projectContext,
    };
  }

  private mapPhase(row: Record<string, unknown>): PlanningPhase {
    let requirements: string[];
    let successCriteria: string[];
    let plan: unknown | null;
    try {
      requirements = JSON.parse(row["requirements"] as string) as string[];
      successCriteria = JSON.parse(row["success_criteria"] as string) as string[];
      plan = row["plan"] ? (JSON.parse(row["plan"] as string) as unknown) : null;
    } catch (cause) {
      throw new PlanningError("Corrupt JSON in planning_phases", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause,
      });
    }
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      name: row["name"] as string,
      goal: row["goal"] as string,
      requirements,
      successCriteria,
      plan,
      status: row["status"] as PlanningPhase["status"],
      phaseOrder: row["phase_order"] as number,
      verificationResult: row["verification_result"]
        ? (JSON.parse(row["verification_result"] as string) as VerificationResult)
        : null,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
    };
  }

  private mapRequirement(row: Record<string, unknown>): PlanningRequirement {
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phaseId: (row["phase_id"] as string | null) ?? null,
      reqId: row["req_id"] as string,
      description: row["description"] as string,
      category: row["category"] as string,
      tier: row["tier"] as PlanningRequirement["tier"],
      status: row["status"] as PlanningRequirement["status"],
      rationale: (row["rationale"] as string | null) ?? null,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
    };
  }

  private mapCheckpoint(row: Record<string, unknown>): PlanningCheckpoint {
    let context: Record<string, unknown>;
    try {
      context = JSON.parse(row["context"] as string) as Record<string, unknown>;
    } catch (cause) {
      throw new PlanningError("Corrupt context JSON in planning_checkpoints", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause,
      });
    }
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      type: row["type"] as string,
      question: row["question"] as string,
      decision: (row["decision"] as string | null) ?? null,
      context,
      createdAt: row["created_at"] as string,
    };
  }

  private mapResearch(row: Record<string, unknown>): PlanningResearch {
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phase: row["phase"] as string,
      dimension: row["dimension"] as string,
      content: row["content"] as string,
      status: (row["status"] as "complete" | "partial" | "failed") ?? "complete",
      createdAt: row["created_at"] as string,
    };
  }

  private mapSpawnRecord(row: Record<string, unknown>): SpawnRecord {
    const wave = row["wave"] as number;
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phaseId: row["phase_id"] as string,
      agentSessionId: row["agent_session_id"] as string,
      status: row["status"] as SpawnRecord["status"],
      result: (row["result"] as string | null) ?? null,
      wave,
      waveNumber: wave,
      startedAt: (row["started_at"] as string | null) ?? null,
      completedAt: (row["completed_at"] as string | null) ?? null,
      errorMessage: (row["error_message"] as string | null) ?? null,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
    };
  }
}
