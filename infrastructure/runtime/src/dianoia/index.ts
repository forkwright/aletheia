// dianoia — planning module public API
export { PlanningStore } from "./store.js";
export { DianoiaOrchestrator } from "./orchestrator.js";
export { detectPlanningIntent } from "./intent.js";
export type {
  DianoiaState,
  PlanningCheckpoint,
  PlanningConfig,
  PlanningPhase,
  PlanningProject,
  PlanningRequirement,
  PlanningResearch,
  ProjectContext,
} from "./types.js";
export { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION } from "./schema.js";
export { ResearchOrchestrator } from "./researcher.js";
export { createPlanResearchTool } from "./research-tool.js";
export { transition, VALID_TRANSITIONS } from "./machine.js";
export type { PlanningEvent } from "./machine.js";
export { RequirementsOrchestrator } from "./requirements.js";
export type { CategoryProposal, FeatureProposal, ScopingDecision } from "./requirements.js";
export { createPlanRequirementsTool } from "./requirements-tool.js";
export { RoadmapOrchestrator } from "./roadmap.js";
export type { PhaseDefinition, PhasePlan, PlanStep } from "./roadmap.js";
export { createPlanRoadmapTool } from "./roadmap-tool.js";
export { ExecutionOrchestrator } from "./execution.js";
export type { ExecutionSnapshot, PlanEntry } from "./execution.js";
export { createPlanExecuteTool } from "./execution-tool.js";

// Verification
export { GoalBackwardVerifier } from "./verifier.js";
export type { VerificationGap, VerificationStatus, VerificationResult } from "./types.js";
export { createPlanVerifyTool } from "./verifier-tool.js";

// Checkpoint
export { CheckpointSystem } from "./checkpoint.js";
export type { TrueBlockerCategory } from "./checkpoint.js";
