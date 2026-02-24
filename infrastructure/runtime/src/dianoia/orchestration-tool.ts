// orchestration_tool — unified interface for all Orchestration Core capabilities
// Provides access to state transitions, execution tracking, verification analysis,
// rollback planning, and discussion question generation

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { OrchestrationCore } from "./orchestration-core.js";
import { PlanningStore } from "./store.js";
import type { VerificationResult } from "./types.js";

const log = createLogger("dianoia:orchestration-tool");

export function createOrchestrationTool(db: Database.Database): ToolHandler {
  const orchestrator = new OrchestrationCore(db);
  const store = new PlanningStore(db);

  return {
    definition: {
      name: "orchestration_manage",
      description: [
        "Manage all aspects of the Dianoia planning orchestration core.",
        "Capabilities:",
        "- state_transition: Execute and validate state machine transitions",
        "- execution_status: Get wave-based execution status and phase tracking",
        "- verify_analysis: Analyze verification results and categorize gaps",
        "- rollback_plan: Generate comprehensive rollback plans for failed phases",
        "- discussion_questions: Generate context-aware discussion questions",
        "- integration_report: Comprehensive status report across all orchestration aspects"
      ].join('\n'),
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: [
              "state_transition",
              "execution_status", 
              "verify_analysis",
              "rollback_plan",
              "discussion_questions",
              "integration_report"
            ],
            description: "Orchestration action to perform"
          },
          projectId: {
            type: "string",
            description: "Planning project ID (required for all actions)"
          },
          event: {
            type: "string",
            description: "State machine event (required for state_transition)"
          },
          metadata: {
            type: "object", 
            description: "Additional metadata for state transitions"
          },
          phaseId: {
            type: "string",
            description: "Phase ID (required for verify_analysis, rollback_plan, discussion_questions)"
          },
          verificationResult: {
            type: "object",
            description: "Verification result object (required for verify_analysis, rollback_plan)"
          },
          failureReason: {
            type: "string",
            description: "Reason for phase failure (optional for rollback_plan)"
          }
        },
        required: ["action", "projectId"]
      }
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const action = input.action as string;
      const projectId = input.projectId as string;

      try {
        // Validate project exists
        const project = store.getProject(projectId);
        if (!project) {
          return JSON.stringify({
            error: `Project ${projectId} not found`
          });
        }

        switch (action) {
          case "state_transition": {
            const event = input.event as string;
            if (!event) {
              return JSON.stringify({
                error: "Event parameter required for state_transition"
              });
            }

            const metadata = input.metadata as Record<string, unknown> || {};
            const result = orchestrator.executeStateTransition(projectId, event, metadata);

            return JSON.stringify({
              action: "state_transition",
              projectId,
              result,
              success: result.success,
              message: result.success 
                ? `State transition: ${result.fromState} → ${result.toState}`
                : `Transition failed: ${result.metadata?.error}`
            }, null, 2);
          }

          case "execution_status": {
            const status = orchestrator.getExecutionStatus(projectId);
            const phases = store.listPhases(projectId);
            
            return JSON.stringify({
              action: "execution_status",
              projectId,
              project: {
                goal: project.goal,
                state: project.state,
                totalPhases: phases.length
              },
              execution: status,
              phases: phases.map(p => ({
                id: p.id,
                name: p.name,
                status: p.status,
                phaseOrder: p.phaseOrder
              })),
              summary: {
                progress: `${status.completedPhases.length}/${phases.length} phases complete`,
                currentState: project.state,
                currentWave: status.currentWave >= 0 ? `${status.currentWave + 1}/${status.totalWaves}` : "N/A"
              }
            }, null, 2);
          }

          case "verify_analysis": {
            const phaseId = input.phaseId as string;
            const verificationResult = input.verificationResult as VerificationResult;
            
            if (!phaseId) {
              return JSON.stringify({ error: "phaseId required for verify_analysis" });
            }
            if (!verificationResult) {
              return JSON.stringify({ error: "verificationResult required for verify_analysis" });
            }

            const analysis = orchestrator.analyzeVerificationResult(
              projectId,
              phaseId,
              verificationResult
            );

            const phase = store.getPhase(phaseId);
            
            return JSON.stringify({
              action: "verify_analysis",
              projectId,
              phaseId,
              phaseName: phase?.name || "Unknown",
              analysis,
              verificationResult,
              summary: {
                status: analysis.overallStatus,
                totalGaps: analysis.criticalGaps.length + analysis.minorGaps.length,
                criticalGaps: analysis.criticalGaps.length,
                minorGaps: analysis.minorGaps.length,
                nextAction: analysis.nextActions[0] || "No action needed"
              }
            }, null, 2);
          }

          case "rollback_plan": {
            const phaseId = input.phaseId as string;
            const verificationResult = input.verificationResult as VerificationResult;
            const failureReason = input.failureReason as string || "Phase verification failed";
            
            if (!phaseId) {
              return JSON.stringify({ error: "phaseId required for rollback_plan" });
            }
            if (!verificationResult) {
              return JSON.stringify({ error: "verificationResult required for rollback_plan" });
            }

            const rollbackPlan = orchestrator.generateRollbackPlan(
              projectId,
              phaseId,
              verificationResult,
              failureReason
            );

            const phase = store.getPhase(phaseId);
            
            return JSON.stringify({
              action: "rollback_plan",
              projectId,
              phaseId,
              phaseName: phase?.name || "Unknown",
              rollbackPlan,
              summary: {
                failedPhase: phase?.name || "Unknown",
                skippedPhases: rollbackPlan.skippedPhases.length,
                rollbackActions: rollbackPlan.rollbackActions.length,
                hasResumePoint: !!rollbackPlan.resumePoint,
                actionsByType: rollbackPlan.rollbackActions.reduce((acc, action) => {
                  acc[action.type] = (acc[action.type] || 0) + 1;
                  return acc;
                }, {} as Record<string, number>)
              }
            }, null, 2);
          }

          case "discussion_questions": {
            const phaseId = input.phaseId as string;
            
            if (!phaseId) {
              return JSON.stringify({ error: "phaseId required for discussion_questions" });
            }

            const questions = orchestrator.generateDiscussionQuestions(projectId, phaseId);
            const phase = store.getPhase(phaseId);
            
            return JSON.stringify({
              action: "discussion_questions",
              projectId,
              phaseId,
              phaseName: phase?.name || "Unknown",
              questions,
              summary: {
                totalQuestions: questions.length,
                highPriority: questions.filter(q => q.priority === "high").length,
                mediumPriority: questions.filter(q => q.priority === "medium").length,
                lowPriority: questions.filter(q => q.priority === "low").length,
                categories: [...new Set(questions.map(q => q.category))]
              }
            }, null, 2);
          }

          case "integration_report": {
            // Comprehensive report across all orchestration capabilities
            const phases = store.listPhases(projectId);
            const executionStatus = orchestrator.getExecutionStatus(projectId);
            
            // Check for verification results
            const phasesWithVerification = phases.filter(p => p.verificationResult);
            const failedVerifications = phasesWithVerification.filter(p => 
              p.verificationResult && p.verificationResult.status !== "met"
            );

            // Generate sample discussion questions for pending phases
            const pendingPhases = phases.filter(p => p.status === "pending").slice(0, 3);
            const sampleQuestions = pendingPhases.map(phase => ({
              phaseId: phase.id,
              phaseName: phase.name,
              questionCount: orchestrator.generateDiscussionQuestions(projectId, phase.id).length
            }));

            return JSON.stringify({
              action: "integration_report",
              projectId,
              timestamp: new Date().toISOString(),
              project: {
                goal: project.goal,
                state: project.state,
                created: project.createdAt,
                updated: project.updatedAt
              },
              orchestration: {
                stateTransitions: {
                  currentState: project.state,
                  validNextEvents: getValidEvents(project.state)
                },
                executionTracking: executionStatus,
                verification: {
                  phasesWithResults: phasesWithVerification.length,
                  failedVerifications: failedVerifications.length,
                  phases: phasesWithVerification.map(p => ({
                    id: p.id,
                    name: p.name,
                    status: p.verificationResult?.status,
                    gapCount: p.verificationResult?.gaps?.length || 0
                  }))
                },
                rollbackPlanning: {
                  phasesNeedingRollback: failedVerifications.length,
                  potentialSkippedPhases: failedVerifications.reduce((count, phase) => {
                    if (phase.verificationResult) {
                      const plan = orchestrator.generateRollbackPlan(
                        projectId,
                        phase.id,
                        phase.verificationResult
                      );
                      return count + plan.skippedPhases.length;
                    }
                    return count;
                  }, 0)
                },
                discussionQuestions: {
                  sampledPhases: sampleQuestions,
                  totalSampleQuestions: sampleQuestions.reduce((sum, p) => sum + p.questionCount, 0)
                }
              },
              summary: {
                overallHealth: determineOverallHealth(project, executionStatus, failedVerifications.length),
                criticalIssues: failedVerifications.length,
                phasesAtRisk: executionStatus.failedPhases.length + executionStatus.skippedPhases.length,
                nextSteps: getNextSteps(project, executionStatus)
              }
            }, null, 2);
          }

          default:
            return JSON.stringify({
              error: `Unknown orchestration action: ${action}`
            });
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        log.error(`Orchestration action [${action}] failed: ${message}`);
        
        return JSON.stringify({
          error: `Orchestration action failed: ${message}`,
          action,
          projectId
        });
      }
    }
  };
}

function getValidEvents(state: string): string[] {
  // This should match the VALID_TRANSITIONS from machine.ts
  const transitions: Record<string, string[]> = {
    idle: ["START_QUESTIONING", "ABANDON"],
    questioning: ["START_RESEARCH", "ABANDON"],
    researching: ["RESEARCH_COMPLETE", "BLOCK", "ABANDON"],
    requirements: ["REQUIREMENTS_COMPLETE", "ABANDON"],
    roadmap: ["ROADMAP_COMPLETE", "ABANDON"],
    discussing: ["DISCUSSION_COMPLETE", "ABANDON"],
    "phase-planning": ["PLAN_READY", "ABANDON"],
    executing: ["VERIFY", "BLOCK", "ABANDON"],
    verifying: ["NEXT_PHASE", "ALL_PHASES_COMPLETE", "PHASE_FAILED", "ABANDON"],
    blocked: ["RESUME", "ABANDON"],
    complete: [],
    abandoned: []
  };
  
  return transitions[state] || [];
}

function determineOverallHealth(
  project: any,
  executionStatus: any,
  failedVerifications: number
): "healthy" | "warning" | "critical" {
  if (failedVerifications > 0 || executionStatus.failedPhases.length > 0) {
    return "critical";
  }
  
  if (project.state === "blocked" || executionStatus.skippedPhases.length > 0) {
    return "warning";
  }
  
  return "healthy";
}

function getNextSteps(project: any, executionStatus: any): string[] {
  const steps: string[] = [];
  
  if (project.state === "blocked") {
    steps.push("Resolve blocking issues and resume execution");
  }
  
  if (executionStatus.failedPhases.length > 0) {
    steps.push("Review and retry failed phases");
  }
  
  if (executionStatus.runningPhases.length > 0) {
    steps.push("Monitor running phases for completion");
  }
  
  if (executionStatus.pendingPhases.length > 0 && steps.length === 0) {
    steps.push("Continue with next wave of phase execution");
  }
  
  if (project.state === "complete") {
    steps.push("Project complete - consider retrospective analysis");
  }
  
  if (steps.length === 0) {
    steps.push("No immediate action required");
  }
  
  return steps;
}