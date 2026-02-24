// TypeScript interfaces for the dianoia (planning) module
import type { PlanningConfigSchema } from "../taxis/schema.js";

export type PlanningConfig = PlanningConfigSchema;

export interface ProjectContext {
  goal?: string;
  coreValue?: string;
  constraints?: string[];
  keyDecisions?: string[];
  rawTranscript?: Array<{ turn: number; text: string }>;
}

export type DianoiaState =
  | "idle"
  | "questioning"
  | "researching"
  | "requirements"
  | "roadmap"
  | "phase-planning"
  | "executing"
  | "verifying"
  | "complete"
  | "blocked"
  | "abandoned";

export interface PlanningProject {
  id: string;
  nousId: string;
  sessionId: string;
  goal: string;
  state: DianoiaState;
  config: PlanningConfig;
  contextHash: string;
  createdAt: string;
  updatedAt: string;
  projectContext: ProjectContext | null;
}

export interface PlanningPhase {
  id: string;
  projectId: string;
  name: string;
  goal: string;
  requirements: string[];
  successCriteria: string[];
  plan: unknown | null;
  status: "pending" | "executing" | "complete" | "failed" | "skipped";
  phaseOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface PlanningRequirement {
  id: string;
  projectId: string;
  phaseId: string | null;
  reqId: string;
  description: string;
  category: string;
  tier: "v1" | "v2" | "out-of-scope";
  status: "pending" | "validated" | "skipped";
  // Only meaningful when tier is "out-of-scope" — explains why the requirement was deferred
  rationale: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface PlanningCheckpoint {
  id: string;
  projectId: string;
  type: string;
  question: string;
  decision: string | null;
  context: Record<string, unknown>;
  createdAt: string;
}

export interface PlanningResearch {
  id: string;
  projectId: string;
  phase: string;
  dimension: string;
  content: string;
  status: "complete" | "partial" | "failed";
  createdAt: string;
}
