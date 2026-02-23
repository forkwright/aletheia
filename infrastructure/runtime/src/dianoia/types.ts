// TypeScript interfaces for the dianoia (planning) module
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

export interface PlanningConfig {
  depth: "quick" | "standard" | "comprehensive";
  parallelization: boolean;
  research: boolean;
  plan_check: boolean;
  verifier: boolean;
  mode: "yolo" | "interactive";
}

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
  createdAt: string;
}
