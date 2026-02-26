// dianoia — planning module public API
export { PlanningStore } from "./store.js";
export { DianoiaOrchestrator } from "./orchestrator.js";
export { detectPlanningIntent } from "./intent.js";
export type {
  DianoiaState,
  PlanningCheckpoint,
  PlanningConfig,
  PlanningMessage,
  PlanningPhase,
  PlanningProject,
  PlanningRequirement,
  PlanningResearch,
  ProjectContext,
} from "./types.js";
export { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION, PLANNING_V28_MIGRATION, PLANNING_V29_MIGRATION } from "./schema.js";
export type { DiscussionQuestion, DiscussionOption, PlanningDecision, TurnCount } from "./types.js";
export { ResearchOrchestrator } from "./researcher.js";
export { createPlanResearchTool } from "./research-tool.js";
export { transition, VALID_TRANSITIONS } from "./machine.js";
export type { PlanningEvent } from "./machine.js";
export { RequirementsOrchestrator } from "./requirements.js";
export type { CategoryProposal, FeatureProposal, ScopingDecision } from "./requirements.js";
export { createPlanRequirementsTool } from "./requirements-tool.js";

// Phase 6+: Roadmap, Execution, Verification, Checkpoints
export { RoadmapOrchestrator } from "./roadmap.js";
export { createPlanRoadmapTool } from "./roadmap-tool.js";
export { ExecutionOrchestrator } from "./execution.js";
export type { MessageDelivery } from "./execution.js";
export { createPlanExecuteTool } from "./execution-tool.js";
export { createPlanInterjectTool } from "./interject-tool.js";
export { GoalBackwardVerifier } from "./verifier.js";
export { createPlanVerifyTool } from "./verifier-tool.js";
export { CheckpointSystem } from "./checkpoint.js";
export { createPlanCreateTool } from "./create-tool.js";
export {
  getProjectDir,
  getPhaseDir,
  ensureProjectDir,
  ensurePhaseDir,
  writeProjectFile,
  writeRequirementsFile,
  writeResearchFile,
  writeRoadmapFile,
  writeDiscussFile,
  writePlanFile,
  writeStateFile,
  writeVerifyFile,
  readProjectFile,
  readRequirementsFile,
  readRoadmapFile,
  readResearchFile,
  readDiscussFile,
  readPlanFile,
} from "./project-files.js";

// Retrospective (Spec 32 Phase 4)
export { RetrospectiveGenerator } from "./retrospective.js";
export type { RetrospectiveEntry, PhaseRetrospective, Pattern } from "./retrospective.js";

// Discussion tool (Spec 32 Phase 3)
export { createPlanDiscussTool } from "./discuss-tool.js";

// Context engineering (Spec 32 Phase 2)
export { buildContextPacket, selectModelForRole, modelTierToRole } from "./context-packet.js";
export type { SubAgentRole, ContextPacketOptions, ModelTier } from "./context-packet.js";

// Orchestration Core (Improving Dianoia - ORCH requirements)
export { OrchestrationCore } from "./orchestration-core.js";
export type { RollbackPlan, RollbackAction, StateTransitionResult } from "./orchestration-core.js";
export { createOrchestrationTool } from "./orchestration-tool.js";

// State Foundation (ENG-01/02/08/12)
export { StateReconciler } from "./state-reconciler.js";
export type { ReconciliationResult, ReconciliationSummary, StepBoundaryInfo } from "./state-reconciler.js";
export { writeHandoffFile, readHandoffFile, clearHandoffFile, discoverHandoffs, buildHandoffState } from "./handoff.js";
export type { HandoffState } from "./handoff.js";
export { calculateBudgetAllocation, buildOrchestratorContext, checkBudget, DEFAULT_ORCHESTRATOR_CEILING } from "./context-budget.js";
export type { BudgetAllocation } from "./context-budget.js";
export { writeStructuredDiscussFile, readStructuredDiscussFile, extractDecisionsFromQuestions, createEmptyArtifact, acquireDiscussionLock, releaseDiscussionLock, isDiscussionLocked } from "./discussion-artifacts.js";
export type { DiscussionArtifact, BoundaryItem, ImplementationDecision, DiscretionItem, DeferredIdea } from "./discussion-artifacts.js";

// Research & Standards (ENG-10/11/15)
export { generateCodebaseMap, scanDirectory, writeCodebaseMapFile, detectLanguage, extractImports, extractExports, groupIntoModules, detectLayers, detectConventions } from "./codebase-map.js";
export type { CodebaseMapResult, ModuleInfo, ArchitecturalLayer, Convention, FileInfo } from "./codebase-map.js";
export { RESEARCH_LEVELS, extractComplexitySignals, selectResearchLevel, getResearchConfig, determineResearchLevel } from "./research-levels.js";
export type { ResearchLevel, ResearchLevelConfig } from "./research-levels.js";
export { getLanguageRules, buildStandards, writeStandardsFile, readStandardsFile, createUserPreferenceRule } from "./coding-standards.js";
export type { ProjectStandards, CodingRule, StandardsLayer } from "./coding-standards.js";
export { createAdhocWork, ADHOC_TOOL_DEFINITION } from "./adhoc-tool.js";
export type { AdhocWorkRequest, AdhocWorkResult } from "./adhoc-tool.js";
export { FileSyncDaemon } from "./file-sync.js";
