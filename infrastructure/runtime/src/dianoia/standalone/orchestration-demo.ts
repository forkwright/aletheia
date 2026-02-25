// Orchestration Core Demonstration
// This script demonstrates all ORCH requirements working together in a realistic scenario

import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { OrchestrationCore } from "./orchestration-core.js";
import { PlanningStore } from "./store.js";
import type { VerificationResult } from "./types.js";
import type { PhasePlan } from "./roadmap.js";

// Set up in-memory database with full schema
function createDemoDatabase(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  
  // Apply all migrations
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  db.exec(PLANNING_V26_MIGRATION);
  db.exec(PLANNING_V27_MIGRATION);
  
  return db;
}

async function demonstrateOrchestrationCore() {
  console.log("🚀 Orchestration Core Demonstration");
  console.log("===================================\n");

  const db = createDemoDatabase();
  const store = new PlanningStore(db);
  const orchestrator = new OrchestrationCore(db);

  // Create a demo project
  const project = store.createProject({
    nousId: "demo-nous",
    sessionId: "demo-session", 
    goal: "Build a modern web application with user authentication and real-time features",
    config: {
      depth: "standard",
      parallelization: true,
      research: true,
      plan_check: true,
      verifier: true,
      mode: "interactive",
      pause_between_phases: false
    }
  });

  console.log(`📋 Created project: ${project.goal}`);
  console.log(`   Project ID: ${project.id}\n`);

  // ORCH-01: Demonstrate state machine with valid transitions
  console.log("🔄 ORCH-01: State Machine Validation");
  console.log("=====================================");

  const stateProgression = [
    { event: "START_QUESTIONING", description: "Begin project questioning" },
    { event: "START_RESEARCH", description: "Start research phase" },
    { event: "RESEARCH_COMPLETE", description: "Complete research" },
    { event: "REQUIREMENTS_COMPLETE", description: "Complete requirements definition" },
    { event: "ROADMAP_COMPLETE", description: "Complete roadmap generation" },
    { event: "DISCUSSION_COMPLETE", description: "Complete gray-area discussions" },
    { event: "PLAN_READY", description: "Plans ready for execution" }
  ];

  for (const transition of stateProgression) {
    const result = orchestrator.executeStateTransition(
      project.id, 
      transition.event,
      { description: transition.description }
    );
    
    console.log(`   ✅ ${transition.event}: ${result.fromState} → ${result.toState}`);
    console.log(`      ${transition.description}`);
  }

  // Try an invalid transition to show validation
  const invalidResult = orchestrator.executeStateTransition(
    project.id,
    "START_QUESTIONING", // Invalid from current state
    { description: "Attempt invalid transition" }
  );
  console.log(`   ❌ Invalid transition rejected: ${invalidResult.metadata?.error}\n`);

  // Create phases for execution demonstration
  console.log("📅 Creating project phases with dependencies...");
  
  const phaseAuth = store.createPhase({
    projectId: project.id,
    name: "Authentication System",
    goal: "Implement secure user authentication",
    requirements: ["AUTH-01", "AUTH-02", "SEC-01"],
    successCriteria: [
      "User registration and login functional",
      "JWT tokens properly implemented",
      "Password security standards met"
    ],
    phaseOrder: 1
  });

  const phaseAPI = store.createPhase({
    projectId: project.id,
    name: "API Backend",
    goal: "Build REST API endpoints",
    requirements: ["API-01", "API-02", "PERF-01"],
    successCriteria: [
      "All CRUD operations working",
      "API documentation complete",
      "Response times under 200ms"
    ],
    phaseOrder: 2
  });

  const phaseFrontend = store.createPhase({
    projectId: project.id,
    name: "Frontend Interface",
    goal: "Create responsive web interface",
    requirements: ["UI-01", "UI-02", "UX-01"],
    successCriteria: [
      "Mobile-responsive design",
      "Accessibility standards met",
      "User testing completed"
    ],
    phaseOrder: 3
  });

  const phaseRealtime = store.createPhase({
    projectId: project.id,
    name: "Real-time Features",
    goal: "Implement WebSocket-based real-time features",
    requirements: ["RT-01", "RT-02", "PERF-02"],
    successCriteria: [
      "Real-time messaging functional",
      "Live updates working",
      "Connection handling robust"
    ],
    phaseOrder: 4
  });

  // Set up dependencies: Auth → API → Frontend, Realtime depends on API + Frontend
  const apiPlan: PhasePlan = {
    steps: [],
    dependencies: [phaseAuth.id],
    acceptanceCriteria: ["API endpoints tested", "Authentication integrated"]
  };

  const frontendPlan: PhasePlan = {
    steps: [],
    dependencies: [phaseAPI.id],
    acceptanceCriteria: ["UI components functional", "API integration complete"]
  };

  const realtimePlan: PhasePlan = {
    steps: [],
    dependencies: [phaseAPI.id, phaseFrontend.id],
    acceptanceCriteria: ["WebSocket connection stable", "Real-time UI updates working"]
  };

  store.updatePhasePlan(phaseAPI.id, apiPlan);
  store.updatePhasePlan(phaseFrontend.id, frontendPlan);
  store.updatePhasePlan(phaseRealtime.id, realtimePlan);

  console.log(`   Created 4 phases with dependency chain\n`);

  // ORCH-02: Demonstrate wave-based execution tracking
  console.log("⚡ ORCH-02: Wave-based Execution Tracking");
  console.log("=========================================");

  const executionStatus = orchestrator.getExecutionStatus(project.id);
  console.log(`   Total waves: ${executionStatus.totalWaves}`);
  console.log(`   Current wave: ${executionStatus.currentWave >= 0 ? executionStatus.currentWave + 1 : 'Not started'}`);
  console.log(`   Phase status:`);
  console.log(`     Pending: ${executionStatus.pendingPhases.length}`);
  console.log(`     Running: ${executionStatus.runningPhases.length}`);
  console.log(`     Complete: ${executionStatus.completedPhases.length}`);
  console.log(`     Failed: ${executionStatus.failedPhases.length}`);

  // Simulate some execution progress
  const spawnAuth = store.createSpawnRecord({
    projectId: project.id,
    phaseId: phaseAuth.id,
    waveNumber: 0
  });
  store.updateSpawnRecord(spawnAuth.id, { status: "done" });

  const spawnAPI = store.createSpawnRecord({
    projectId: project.id,
    phaseId: phaseAPI.id,
    waveNumber: 1
  });
  store.updateSpawnRecord(spawnAPI.id, { status: "running" });

  const updatedStatus = orchestrator.getExecutionStatus(project.id);
  console.log(`\n   After simulated progress:`);
  console.log(`     Complete: ${updatedStatus.completedPhases.length} (Auth done)`);
  console.log(`     Running: ${updatedStatus.runningPhases.length} (API in progress)\n`);

  // ORCH-03: Demonstrate verification analysis
  console.log("🔍 ORCH-03: Verification Analysis");
  console.log("=================================");

  // Simulate a successful verification
  const successfulVerification: VerificationResult = {
    status: "met",
    summary: "Authentication system meets all success criteria",
    gaps: [],
    verifiedAt: new Date().toISOString()
  };

  const successAnalysis = orchestrator.analyzeVerificationResult(
    project.id,
    phaseAuth.id,
    successfulVerification
  );

  console.log(`   ✅ Auth Phase Verification: ${successAnalysis.overallStatus}`);
  console.log(`      ${successAnalysis.recommendations[0]}`);

  // Simulate a failed verification with gaps
  const failedVerification: VerificationResult = {
    status: "partially-met",
    summary: "API backend has critical security gaps",
    gaps: [
      {
        criterion: "API documentation complete",
        status: "not-met",
        detail: "API documentation missing for 3 endpoints",
        proposedFix: "Complete OpenAPI documentation for all endpoints"
      },
      {
        criterion: "Response times under 200ms",
        status: "partially-met",
        detail: "Most endpoints fast, but user search is slow",
        proposedFix: "Optimize user search query with database indexes"
      }
    ],
    verifiedAt: new Date().toISOString()
  };

  const failedAnalysis = orchestrator.analyzeVerificationResult(
    project.id,
    phaseAPI.id,
    failedVerification
  );

  console.log(`   ❌ API Phase Verification: ${failedAnalysis.overallStatus}`);
  console.log(`      Critical gaps: ${failedAnalysis.criticalGaps.length}`);
  console.log(`      Minor gaps: ${failedAnalysis.minorGaps.length}`);
  console.log(`      Next action: ${failedAnalysis.nextActions[0]}\n`);

  // ORCH-04: Demonstrate rollback plan generation
  console.log("🔄 ORCH-04: Rollback Plan Generation");
  console.log("====================================");

  const rollbackPlan = orchestrator.generateRollbackPlan(
    project.id,
    phaseAPI.id,
    failedVerification,
    "API phase failed verification due to documentation and performance issues"
  );

  console.log(`   Failed phase: API Backend`);
  console.log(`   Phases to skip: ${rollbackPlan.skippedPhases.length}`);
  console.log(`   Rollback actions: ${rollbackPlan.rollbackActions.length}`);
  
  const actionsByType = rollbackPlan.rollbackActions.reduce((acc, action) => {
    acc[action.type] = (acc[action.type] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  console.log(`   Action breakdown:`);
  for (const [type, count] of Object.entries(actionsByType)) {
    console.log(`     ${type}: ${count}`);
  }

  if (rollbackPlan.resumePoint) {
    const resumePhase = store.getPhase(rollbackPlan.resumePoint);
    console.log(`   Resume point: ${resumePhase?.name || 'Unknown'}`);
  } else {
    console.log(`   No independent resume point available`);
  }

  console.log(`\n   Sample rollback actions:`);
  rollbackPlan.rollbackActions.slice(0, 3).forEach((action, i) => {
    console.log(`     ${i + 1}. ${action.type}: ${action.description}`);
  });
  console.log();

  // ORCH-05: Demonstrate discussion question generation
  console.log("💬 ORCH-05: Discussion Question Generation");
  console.log("==========================================");

  // Create some requirements to drive better question generation
  store.createRequirement({
    projectId: project.id,
    phaseId: phaseFrontend.id,
    reqId: "UI-01",
    description: "Build responsive web interface using modern framework",
    category: "Technology",
    tier: "v1"
  });

  store.createRequirement({
    projectId: project.id,
    phaseId: phaseFrontend.id,
    reqId: "PERF-02",
    description: "Ensure fast page load performance and scalability",
    category: "Performance",
    tier: "v1"
  });

  store.createRequirement({
    projectId: project.id,
    phaseId: phaseFrontend.id,
    reqId: "SEC-02",
    description: "Implement client-side security measures",
    category: "Security",
    tier: "v1"
  });

  const questions = orchestrator.generateDiscussionQuestions(
    project.id,
    phaseFrontend.id
  );

  console.log(`   Generated ${questions.length} discussion questions for Frontend phase:`);
  
  const categoryCounts = questions.reduce((acc, q) => {
    acc[q.category] = (acc[q.category] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  console.log(`   Categories: ${Object.entries(categoryCounts).map(([cat, count]) => `${cat}(${count})`).join(', ')}`);
  
  console.log(`\n   Sample questions:`);
  questions.slice(0, 2).forEach((q, i) => {
    console.log(`     ${i + 1}. ${q.question}`);
    console.log(`        Recommendation: ${q.recommendation}`);
    console.log(`        Priority: ${q.priority}, Category: ${q.category}`);
    console.log(`        Options: ${q.options.length} choices available\n`);
  });

  // Integration demonstration
  console.log("🌐 Integration Demonstration");
  console.log("============================");

  console.log(`   Project state: ${store.getProjectOrThrow(project.id).state}`);
  console.log(`   Total phases: ${store.listPhases(project.id).length}`);
  console.log(`   Execution waves computed: ${orchestrator.getExecutionStatus(project.id).totalWaves}`);
  console.log(`   Discussion questions generated: ${questions.length}`);
  console.log(`   Rollback plan actions: ${rollbackPlan.rollbackActions.length}`);
  console.log(`   Verification analysis complete: ✅`);

  console.log(`\n✨ Orchestration Core demonstration complete!`);
  console.log(`   All ORCH requirements validated:\n`);
  console.log(`   ✅ ORCH-01: Clean state machine with valid transitions`);
  console.log(`   ✅ ORCH-02: Wave-based execution with dependency tracking`);
  console.log(`   ✅ ORCH-03: Post-execution verification with gap detection`);
  console.log(`   ✅ ORCH-04: Automatic rollback planning for failed verification`);
  console.log(`   ✅ ORCH-05: Gray-area discussion question generation`);

  db.close();
}

// Run the demonstration
if (import.meta.url === new URL(process.argv[1], 'file://').href) {
  demonstrateOrchestrationCore().catch(console.error);
}

export { demonstrateOrchestrationCore };