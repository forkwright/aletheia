// PlanningStore — SQLite-backed CRUD for all 5 planning tables
import { createHash } from "node:crypto";
import type Database from "better-sqlite3";
import { generateId } from "../koina/crypto.js";
import { PlanningError } from "../koina/errors.js";
import { createLogger } from "../koina/logger.js";
import type {
  DianoiaState,
  DiscussionOption,
  DiscussionQuestion,
  PlanningCheckpoint,
  PlanningConfig,
  PlanningDecision,
  PlanningMessage,
  PlanningPhase,
  PlanningProject,
  PlanningRequirement,
  PlanningResearch,
  ProjectContext,
  SpawnRecord,
  TurnCount,
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
    dependencies?: string[];
  }): PlanningPhase {
    const id = generateId("phase");

    const insert = this.db.transaction(() => {
      this.db
        .prepare(
          `INSERT INTO planning_phases (id, project_id, name, goal, requirements, success_criteria, phase_order, dependencies)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          id,
          opts.projectId,
          opts.name,
          opts.goal,
          JSON.stringify(opts.requirements),
          JSON.stringify(opts.successCriteria),
          opts.phaseOrder,
          JSON.stringify(opts.dependencies ?? []),
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

  updatePhaseDependencies(id: string, dependencies: string[]): void {
    const update = this.db.transaction(() => {
      this.db
        .prepare(
          `UPDATE planning_phases SET dependencies = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
        )
        .run(JSON.stringify(dependencies), id);
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

  /** Update phase metadata (name, goal, success criteria, requirements, order) */
  updatePhase(
    id: string,
    updates: {
      name?: string;
      goal?: string;
      successCriteria?: string[];
      requirements?: string[];
      phaseOrder?: number;
    },
  ): PlanningPhase {
    const update = this.db.transaction(() => {
      const sets: string[] = [];
      const vals: unknown[] = [];
      if (updates.name !== undefined) { sets.push("name = ?"); vals.push(updates.name); }
      if (updates.goal !== undefined) { sets.push("goal = ?"); vals.push(updates.goal); }
      if (updates.successCriteria !== undefined) { sets.push("success_criteria = ?"); vals.push(JSON.stringify(updates.successCriteria)); }
      if (updates.requirements !== undefined) { sets.push("requirements = ?"); vals.push(JSON.stringify(updates.requirements)); }
      if (updates.phaseOrder !== undefined) { sets.push("phase_order = ?"); vals.push(updates.phaseOrder); }
      if (sets.length === 0) return;
      sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
      vals.push(id);
      const result = this.db
        .prepare(`UPDATE planning_phases SET ${sets.join(", ")} WHERE id = ?`)
        .run(...vals);
      if (result.changes === 0) {
        throw new PlanningError(`Planning phase not found: ${id}`, {
          code: "PLANNING_PHASE_NOT_FOUND",
          context: { id },
        });
      }
    });
    update();
    return this.getPhaseOrThrow(id);
  }

  deletePhase(id: string): void {
    const result = this.db.prepare("DELETE FROM planning_phases WHERE id = ?").run(id);
    if (result.changes === 0) {
      throw new PlanningError(`Planning phase not found: ${id}`, {
        code: "PLANNING_PHASE_NOT_FOUND",
        context: { id },
      });
    }
    // Clean up orphaned requirements that pointed to this phase
    this.db.prepare("UPDATE planning_requirements SET phase_id = NULL WHERE phase_id = ?").run(id);
  }

  /** Reorder phases: move phaseId to newOrder, shift others accordingly */
  reorderPhase(projectId: string, phaseId: string, newOrder: number): void {
    const reorder = this.db.transaction(() => {
      const phase = this.getPhaseOrThrow(phaseId);
      if (phase.projectId !== projectId) {
        throw new PlanningError("Phase does not belong to project", {
          code: "PLANNING_PHASE_NOT_FOUND",
          context: { phaseId, projectId },
        });
      }
      const oldOrder = phase.phaseOrder;
      if (oldOrder === newOrder) return;

      if (newOrder > oldOrder) {
        // Moving down: shift items between (old, new] up by 1
        this.db.prepare(
          `UPDATE planning_phases SET phase_order = phase_order - 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE project_id = ? AND phase_order > ? AND phase_order <= ?`
        ).run(projectId, oldOrder, newOrder);
      } else {
        // Moving up: shift items between [new, old) down by 1
        this.db.prepare(
          `UPDATE planning_phases SET phase_order = phase_order + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE project_id = ? AND phase_order >= ? AND phase_order < ?`
        ).run(projectId, newOrder, oldOrder);
      }

      this.db.prepare(
        `UPDATE planning_phases SET phase_order = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`
      ).run(newOrder, phaseId);
    });
    reorder();
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
    updates: {
      tier?: "v1" | "v2" | "out-of-scope";
      rationale?: string | null;
      description?: string;
      category?: string;
      reqId?: string;
      status?: "pending" | "validated" | "skipped";
      phaseId?: string | null;
    },
  ): PlanningRequirement {
    const update = this.db.transaction(() => {
      const sets: string[] = [];
      const vals: unknown[] = [];
      if (updates.tier !== undefined) { sets.push("tier = ?"); vals.push(updates.tier); }
      if (updates.rationale !== undefined) { sets.push("rationale = ?"); vals.push(updates.rationale); }
      if (updates.description !== undefined) { sets.push("description = ?"); vals.push(updates.description); }
      if (updates.category !== undefined) { sets.push("category = ?"); vals.push(updates.category); }
      if (updates.reqId !== undefined) { sets.push("req_id = ?"); vals.push(updates.reqId); }
      if (updates.status !== undefined) { sets.push("status = ?"); vals.push(updates.status); }
      if (updates.phaseId !== undefined) { sets.push("phase_id = ?"); vals.push(updates.phaseId); }
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
    return this.getRequirementOrThrow(id);
  }

  getRequirement(id: string): PlanningRequirement | undefined {
    const row = this.db
      .prepare("SELECT * FROM planning_requirements WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapRequirement(row) : undefined;
  }

  getRequirementByReqId(projectId: string, reqId: string): PlanningRequirement | undefined {
    const row = this.db
      .prepare("SELECT * FROM planning_requirements WHERE project_id = ? AND req_id = ?")
      .get(projectId, reqId) as Record<string, unknown> | undefined;
    return row ? this.mapRequirement(row) : undefined;
  }

  getRequirementOrThrow(id: string): PlanningRequirement {
    const req = this.getRequirement(id);
    if (!req) {
      throw new PlanningError(`Planning requirement not found: ${id}`, {
        code: "PLANNING_REQUIREMENT_NOT_FOUND",
        context: { id },
      });
    }
    return req;
  }

  deleteRequirement(id: string): void {
    const result = this.db.prepare("DELETE FROM planning_requirements WHERE id = ?").run(id);
    if (result.changes === 0) {
      throw new PlanningError(`Planning requirement not found: ${id}`, {
        code: "PLANNING_REQUIREMENT_NOT_FOUND",
        context: { id },
      });
    }
  }

  /** Generate next sequential reqId for a category (e.g., "EDIT" → "EDIT-09") */
  nextReqId(projectId: string, category: string): string {
    const prefix = category.toUpperCase();
    const rows = this.db
      .prepare("SELECT req_id FROM planning_requirements WHERE project_id = ? AND req_id LIKE ?")
      .all(projectId, `${prefix}-%`) as Array<Record<string, unknown>>;
    const nums = rows
      .map(r => {
        const match = (r["req_id"] as string).match(new RegExp(`^${prefix}-(\\d+)$`));
        return match ? parseInt(match[1]!, 10) : 0;
      })
      .filter(n => !isNaN(n));
    const next = nums.length > 0 ? Math.max(...nums) + 1 : 1;
    return `${prefix}-${String(next).padStart(2, "0")}`;
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

  // --- Project Directory ---

  updateProjectDir(id: string, projectDir: string): void {
    const result = this.db
      .prepare(
        `UPDATE planning_projects SET project_dir = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(projectDir, id);
    if (result.changes === 0) {
      throw new PlanningError(`Planning project not found: ${id}`, {
        code: "PLANNING_PROJECT_NOT_FOUND",
        context: { id },
      });
    }
  }

  // --- Discussions (Spec 32) ---

  createDiscussionQuestion(opts: {
    projectId: string;
    phaseId: string;
    question: string;
    options: DiscussionOption[];
    recommendation?: string | null;
  }): DiscussionQuestion {
    const id = generateId("disc");
    const now = new Date().toISOString();

    this.db
      .prepare(
        `INSERT INTO planning_discussions (id, project_id, phase_id, question, options, recommendation, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)`,
      )
      .run(
        id,
        opts.projectId,
        opts.phaseId,
        opts.question,
        JSON.stringify(opts.options),
        opts.recommendation ?? null,
        now,
        now,
      );

    return this.getDiscussionQuestionOrThrow(id);
  }

  answerDiscussionQuestion(id: string, decision: string, userNote?: string | null): void {
    const result = this.db
      .prepare(
        `UPDATE planning_discussions SET decision = ?, user_note = ?, status = 'answered', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(decision, userNote ?? null, id);
    if (result.changes === 0) {
      throw new PlanningError(`Discussion question not found: ${id}`, {
        code: "PLANNING_DISCUSSION_NOT_FOUND",
        context: { id },
      });
    }
  }

  skipDiscussionQuestion(id: string): void {
    const result = this.db
      .prepare(
        `UPDATE planning_discussions SET status = 'skipped', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(id);
    if (result.changes === 0) {
      throw new PlanningError(`Discussion question not found: ${id}`, {
        code: "PLANNING_DISCUSSION_NOT_FOUND",
        context: { id },
      });
    }
  }

  listDiscussionQuestions(projectId: string, phaseId?: string): DiscussionQuestion[] {
    const rows = phaseId
      ? (this.db
          .prepare("SELECT * FROM planning_discussions WHERE project_id = ? AND phase_id = ? ORDER BY created_at ASC")
          .all(projectId, phaseId) as Array<Record<string, unknown>>)
      : (this.db
          .prepare("SELECT * FROM planning_discussions WHERE project_id = ? ORDER BY created_at ASC")
          .all(projectId) as Array<Record<string, unknown>>);
    return rows.map((r) => this.mapDiscussionQuestion(r));
  }

  getPendingDiscussionQuestions(projectId: string, phaseId: string): DiscussionQuestion[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM planning_discussions WHERE project_id = ? AND phase_id = ? AND status = 'pending' ORDER BY created_at ASC",
      )
      .all(projectId, phaseId) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapDiscussionQuestion(r));
  }

  getDiscussionQuestion(id: string): DiscussionQuestion | undefined {
    const row = this.db
      .prepare("SELECT * FROM planning_discussions WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapDiscussionQuestion(row) : undefined;
  }

  getDiscussionQuestionOrThrow(id: string): DiscussionQuestion {
    const q = this.getDiscussionQuestion(id);
    if (!q) {
      throw new PlanningError(`Discussion question not found: ${id}`, {
        code: "PLANNING_DISCUSSION_NOT_FOUND",
        context: { id },
      });
    }
    return q;
  }

  // --- Private mappers ---

  private mapProject(row: Record<string, unknown>): PlanningProject {
    let config: PlanningConfig;
    try {
      config = JSON.parse(row["config"] as string) as PlanningConfig;
    } catch (error) {
      throw new PlanningError("Corrupt config JSON in planning_projects", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause: error,
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
      projectDir: (row["project_dir"] as string | null) ?? null,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
      projectContext,
    };
  }

  private mapPhase(row: Record<string, unknown>): PlanningPhase {
    let requirements: string[];
    let successCriteria: string[];
    let dependencies: string[];
    let plan: unknown | null;
    try {
      requirements = JSON.parse(row["requirements"] as string) as string[];
      successCriteria = JSON.parse(row["success_criteria"] as string) as string[];
      plan = row["plan"] ? (JSON.parse(row["plan"] as string) as unknown) : null;
      dependencies = row["dependencies"]
        ? (JSON.parse(row["dependencies"] as string) as string[])
        : [];
    } catch (error) {
      throw new PlanningError("Corrupt JSON in planning_phases", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause: error,
      });
    }
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      name: row["name"] as string,
      goal: row["goal"] as string,
      requirements,
      successCriteria,
      dependencies,
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
    let dependsOn: string[] = [];
    let blockedBy: string[] = [];
    try {
      if (row["depends_on"]) dependsOn = JSON.parse(row["depends_on"] as string) as string[];
      if (row["blocked_by"]) blockedBy = JSON.parse(row["blocked_by"] as string) as string[];
    } catch {
      // Gracefully handle corrupt JSON — empty deps is safe default
    }
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
      dependsOn,
      blockedBy,
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
    };
  }

  private mapCheckpoint(row: Record<string, unknown>): PlanningCheckpoint {
    let context: Record<string, unknown>;
    try {
      context = JSON.parse(row["context"] as string) as Record<string, unknown>;
    } catch (error) {
      throw new PlanningError("Corrupt context JSON in planning_checkpoints", {
        code: "PLANNING_STATE_CORRUPT",
        context: { id: row["id"] },
        cause: error,
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

  private mapDiscussionQuestion(row: Record<string, unknown>): DiscussionQuestion {
    let options: DiscussionOption[];
    try {
      options = JSON.parse(row["options"] as string) as DiscussionOption[];
    } catch {
      options = [];
    }
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phaseId: row["phase_id"] as string,
      question: row["question"] as string,
      options,
      recommendation: (row["recommendation"] as string | null) ?? null,
      decision: (row["decision"] as string | null) ?? null,
      userNote: (row["user_note"] as string | null) ?? null,
      status: row["status"] as DiscussionQuestion["status"],
      createdAt: row["created_at"] as string,
      updatedAt: row["updated_at"] as string,
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

  // ─── Decision Audit Trail (OBS-03) ────────────────────────

  logDecision(opts: {
    projectId: string;
    phaseId?: string | null;
    source: "user" | "agent" | "checkpoint" | "system";
    type: string;
    summary: string;
    rationale?: string | null;
    context?: Record<string, unknown>;
  }): PlanningDecision {
    const id = generateId("dec");
    this.db.prepare(
      `INSERT INTO planning_decisions (id, project_id, phase_id, source, type, summary, rationale, context)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
    ).run(
      id,
      opts.projectId,
      opts.phaseId ?? null,
      opts.source,
      opts.type,
      opts.summary,
      opts.rationale ?? null,
      JSON.stringify(opts.context ?? {}),
    );
    return this.getDecision(id)!;
  }

  getDecision(id: string): PlanningDecision | undefined {
    const row = this.db.prepare("SELECT * FROM planning_decisions WHERE id = ?").get(id) as Record<string, unknown> | undefined;
    return row ? this.mapDecision(row) : undefined;
  }

  listDecisions(projectId: string, phaseId?: string): PlanningDecision[] {
    if (phaseId) {
      return (this.db.prepare("SELECT * FROM planning_decisions WHERE project_id = ? AND phase_id = ? ORDER BY created_at ASC")
        .all(projectId, phaseId) as Record<string, unknown>[]).map(r => this.mapDecision(r));
    }
    return (this.db.prepare("SELECT * FROM planning_decisions WHERE project_id = ? ORDER BY created_at ASC")
      .all(projectId) as Record<string, unknown>[]).map(r => this.mapDecision(r));
  }

  private mapDecision(row: Record<string, unknown>): PlanningDecision {
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phaseId: (row["phase_id"] as string | null) ?? null,
      source: row["source"] as PlanningDecision["source"],
      type: row["type"] as string,
      summary: row["summary"] as string,
      rationale: (row["rationale"] as string | null) ?? null,
      context: JSON.parse((row["context"] as string) || "{}"),
      createdAt: row["created_at"] as string,
    };
  }

  // ─── Turn Tracking (OBS-05) ───────────────────────────────

  recordTurn(projectId: string, phaseId: string, nousId: string, tokenCount = 0): void {
    this.db.prepare(
      `INSERT INTO planning_turn_counts (project_id, phase_id, nous_id, turn_count, token_count, updated_at)
       VALUES (?, ?, ?, 1, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
       ON CONFLICT (project_id, phase_id, nous_id) DO UPDATE SET
         turn_count = turn_count + 1,
         token_count = token_count + excluded.token_count,
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`
    ).run(projectId, phaseId, nousId, tokenCount);
  }

  getTurnCounts(projectId: string, phaseId?: string): TurnCount[] {
    if (phaseId) {
      return (this.db.prepare("SELECT * FROM planning_turn_counts WHERE project_id = ? AND phase_id = ? ORDER BY turn_count DESC")
        .all(projectId, phaseId) as Record<string, unknown>[]).map(r => this.mapTurnCount(r));
    }
    return (this.db.prepare("SELECT * FROM planning_turn_counts WHERE project_id = ? ORDER BY phase_id, turn_count DESC")
      .all(projectId) as Record<string, unknown>[]).map(r => this.mapTurnCount(r));
  }

  getProjectTurnTotal(projectId: string): { turns: number; tokens: number } {
    const row = this.db.prepare(
      "SELECT COALESCE(SUM(turn_count), 0) as turns, COALESCE(SUM(token_count), 0) as tokens FROM planning_turn_counts WHERE project_id = ?"
    ).get(projectId) as { turns: number; tokens: number };
    return row;
  }

  private mapTurnCount(row: Record<string, unknown>): TurnCount {
    return {
      projectId: row["project_id"] as string,
      phaseId: row["phase_id"] as string,
      nousId: row["nous_id"] as string,
      turnCount: row["turn_count"] as number,
      tokenCount: row["token_count"] as number,
      updatedAt: row["updated_at"] as string,
    };
  }

  // ─── Message Queue (INTERJ-01/02) ─────────────────────────

  /**
   * Enqueue a message for injection into a running execution.
   * Messages are consumed at turn boundaries (between waves or between tasks).
   */
  enqueueMessage(opts: {
    projectId: string;
    phaseId?: string;
    source: PlanningMessage["source"];
    sourceSessionId?: string;
    content: string;
    priority?: PlanningMessage["priority"];
    expiresAt?: string;
  }): PlanningMessage {
    const id = generateId("msg");
    this.db.prepare(
      `INSERT INTO planning_messages (id, project_id, phase_id, source, source_session_id, content, priority, status, expires_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?)`
    ).run(
      id,
      opts.projectId,
      opts.phaseId ?? null,
      opts.source,
      opts.sourceSessionId ?? null,
      opts.content,
      opts.priority ?? "normal",
      opts.expiresAt ?? null,
    );
    return this.getMessage(id)!;
  }

  getMessage(id: string): PlanningMessage | undefined {
    const row = this.db.prepare("SELECT * FROM planning_messages WHERE id = ?").get(id) as Record<string, unknown> | undefined;
    return row ? this.mapMessage(row) : undefined;
  }

  /**
   * Drain all pending messages for a project, optionally filtered by phase.
   * Marks consumed messages as 'delivered'. Respects priority ordering (critical first).
   * Automatically expires messages past their expiresAt timestamp.
   */
  drainMessages(projectId: string, phaseId?: string): PlanningMessage[] {
    const now = new Date().toISOString();

    // Expire old messages first
    this.db.prepare(
      `UPDATE planning_messages SET status = 'expired'
       WHERE project_id = ? AND status = 'pending' AND expires_at IS NOT NULL AND expires_at < ?`
    ).run(projectId, now);

    // Fetch pending messages (priority: critical > high > normal > low)
    const priorityOrder = "CASE priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'normal' THEN 2 WHEN 'low' THEN 3 END";
    let rows: Record<string, unknown>[];
    if (phaseId) {
      rows = this.db.prepare(
        `SELECT * FROM planning_messages
         WHERE project_id = ? AND (phase_id = ? OR phase_id IS NULL) AND status = 'pending'
         ORDER BY ${priorityOrder}, created_at ASC`
      ).all(projectId, phaseId) as Record<string, unknown>[];
    } else {
      rows = this.db.prepare(
        `SELECT * FROM planning_messages
         WHERE project_id = ? AND status = 'pending'
         ORDER BY ${priorityOrder}, created_at ASC`
      ).all(projectId) as Record<string, unknown>[];
    }

    const messages = rows.map(r => this.mapMessage(r));

    // Mark all drained messages as delivered
    if (messages.length > 0) {
      const ids = messages.map(m => m.id);
      const placeholders = ids.map(() => "?").join(", ");
      this.db.prepare(
        `UPDATE planning_messages SET status = 'delivered', delivered_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id IN (${placeholders})`
      ).run(...ids);
    }

    return messages;
  }

  /**
   * List all messages for a project (all statuses), optionally filtered by phase.
   */
  listMessages(projectId: string, opts?: { phaseId?: string; status?: PlanningMessage["status"] }): PlanningMessage[] {
    let sql = "SELECT * FROM planning_messages WHERE project_id = ?";
    const params: unknown[] = [projectId];
    if (opts?.phaseId) {
      sql += " AND (phase_id = ? OR phase_id IS NULL)";
      params.push(opts.phaseId);
    }
    if (opts?.status) {
      sql += " AND status = ?";
      params.push(opts.status);
    }
    sql += " ORDER BY created_at DESC";
    return (this.db.prepare(sql).all(...params) as Record<string, unknown>[]).map(r => this.mapMessage(r));
  }

  /**
   * Count pending messages for a project — lightweight check for the execution loop.
   */
  countPendingMessages(projectId: string): number {
    const row = this.db.prepare(
      "SELECT COUNT(*) as count FROM planning_messages WHERE project_id = ? AND status = 'pending'"
    ).get(projectId) as { count: number };
    return row.count;
  }

  private mapMessage(row: Record<string, unknown>): PlanningMessage {
    return {
      id: row["id"] as string,
      projectId: row["project_id"] as string,
      phaseId: (row["phase_id"] as string | null) ?? null,
      source: row["source"] as PlanningMessage["source"],
      sourceSessionId: (row["source_session_id"] as string | null) ?? null,
      content: row["content"] as string,
      priority: row["priority"] as PlanningMessage["priority"],
      status: row["status"] as PlanningMessage["status"],
      deliveredAt: (row["delivered_at"] as string | null) ?? null,
      expiresAt: (row["expires_at"] as string | null) ?? null,
      createdAt: row["created_at"] as string,
    };
  }
}
