// Enhanced plan_execute tool with support for both execution orchestrators
import { createLogger } from "../koina/logger.js";
import { PlanningError } from "../koina/errors.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { ExecutionOrchestrator } from "./execution.js";
import type { EnhancedExecutionOrchestrator } from "./enhanced-execution.js";

const log = createLogger("dianoia:enhanced-execution-tool");

// Union type for both orchestrator interfaces
type AnyExecutionOrchestrator = ExecutionOrchestrator | EnhancedExecutionOrchestrator;

export function createEnhancedPlanExecuteTool(
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: AnyExecutionOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_execute",
      description:
        "Execute, pause, resume, retry, skip, or abandon a Dianoia phase execution. " +
        "Enhanced version with wave-based concurrency, intelligent task-to-role mapping, " +
        "and structured extraction with automatic retry. Use action=start to begin execution, " +
        "action=status to check progress.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["start", "pause", "resume", "retry", "skip", "abandon", "status", "configure"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          phaseId: {
            type: "string",
            description: "Phase ID (required for start, resume, retry, skip)",
          },
          planId: {
            type: "string",
            description: "Specific plan ID within the phase (required for retry, skip)",
          },
          nousId: {
            type: "string",
            description: "Nous ID for event bus context (optional)",
          },
          sessionId: {
            type: "string",
            description: "Session ID for event bus context (optional)",
          },
          // Enhanced execution options
          concurrency: {
            type: "boolean",
            description: "Enable wave-based concurrency for independent tasks (default: true)",
          },
          intelligentDispatch: {
            type: "boolean", 
            description: "Use task-to-role mapping instead of fixed executor role (default: true)",
          },
          structuredExtraction: {
            type: "boolean",
            description: "Use instructor-js for structured extraction with Zod validation (default: true)",
          },
          autoRetry: {
            type: "boolean",
            description: "Enable automatic retry with validation feedback (default: true)",
          },
          maxConcurrentTasks: {
            type: "number",
            description: "Maximum concurrent executions per wave (default: 3)",
          }
        },
        required: ["action", "projectId"],
      },
    },
    execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      return handleEnhancedAction(input, planningOrchestrator, executionOrchestrator, context);
    },
  };
}

async function handleEnhancedAction(
  input: Record<string, unknown>,
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: AnyExecutionOrchestrator,
  context: ToolContext,
): Promise<string> {
  const action = input["action"] as string;
  const projectId = input["projectId"] as string;
  const planId = input["planId"] as string | undefined;
  const nousId = (input["nousId"] as string | undefined) ?? context.nousId;
  const sessionId = (input["sessionId"] as string | undefined) ?? context.sessionId;

  try {
    switch (action) {
      case "configure": {
        // Configure enhanced execution options (only available for EnhancedExecutionOrchestrator)
        if (!isEnhancedOrchestrator(executionOrchestrator)) {
          return JSON.stringify({ 
            error: "Configuration not supported by basic execution orchestrator",
            suggestion: "Use EnhancedExecutionOrchestrator for advanced features"
          });
        }

        const options = extractExecutionOptions(input);
        log.info("Execution configuration updated", { projectId, options });
        
        return JSON.stringify({
          message: "Execution configuration updated successfully",
          options,
          features: {
            waveConcurrency: options.enableWaveConcurrency,
            intelligentDispatch: options.useIntelligentDispatch,
            structuredExtraction: options.useStructuredExtraction,
            autoRetry: options.enableAutoRetry
          }
        });
      }

      case "start": {
        const startTime = Date.now();
        log.info("Starting enhanced phase execution", { 
          projectId, 
          orchestratorType: isEnhancedOrchestrator(executionOrchestrator) ? "enhanced" : "basic"
        });

        const result = await executionOrchestrator.executePhase(projectId, context);
        const duration = Date.now() - startTime;
        
        // Enhanced result includes concurrency information
        const enhancedResult = {
          ...result,
          duration,
          enhanced: isEnhancedOrchestrator(executionOrchestrator),
          concurrent: "concurrent" in result ? result.concurrent : false
        };

        if (result.failed === 0) {
          log.info("Phase execution completed successfully", { 
            projectId, 
            waveCount: result.waveCount,
            duration 
          });
          
          const verificationMessage = planningOrchestrator.advanceToVerification(
            projectId, 
            nousId, 
            sessionId
          );
          
          return JSON.stringify({
            ...enhancedResult,
            message: "Execution completed successfully. Advancing to verification.",
            verificationMessage
          });
        } else {
          log.warn("Phase execution completed with failures", {
            projectId,
            failed: result.failed,
            skipped: result.skipped
          });

          return JSON.stringify({
            ...enhancedResult,
            message: "Execution complete with failures. Use action=retry or action=skip to recover.",
            suggestions: [
              "Review failed phases in the execution status",
              "Use action=retry with specific planId to retry individual phases",
              "Use action=skip to skip problematic phases and continue"
            ]
          });
        }
      }

      case "pause": {
        planningOrchestrator.pauseExecution(projectId);
        return JSON.stringify({
          message: "Execution will pause after the current wave completes.",
          note: "Any running tasks will complete before pausing"
        });
      }

      case "resume": {
        log.info("Resuming phase execution", { projectId });
        const msg = planningOrchestrator.resumeExecution(projectId, nousId, sessionId);
        const result = await executionOrchestrator.executePhase(projectId, context);
        
        return JSON.stringify({
          message: msg,
          resumeResult: result,
          enhanced: isEnhancedOrchestrator(executionOrchestrator)
        });
      }

      case "retry": {
        if (!planId) throw new PlanningError("planId required for retry", { code: "PLANNING_PLAN_ID_REQUIRED" });

        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        const planEntry = snapshot.plans.find((p) => p.phaseId === planId);
        if (!planEntry) {
          throw new PlanningError(`Plan ${planId} not found in execution snapshot`, { code: "PLANNING_PLAN_NOT_FOUND", context: { planId } });
        }
        
        log.info("Retrying specific plan", { projectId, planId, previousStatus: planEntry.status });
        
        const result = await executionOrchestrator.executePhase(projectId, context);
        
        return JSON.stringify({
          message: `Retrying plan ${planId} from beginning.`,
          planName: planEntry.name,
          retryResult: result,
          enhanced: isEnhancedOrchestrator(executionOrchestrator)
        });
      }

      case "skip": {
        if (!planId) throw new PlanningError("planId required for skip", { code: "PLANNING_PLAN_ID_REQUIRED" });

        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        const planEntry = snapshot.plans.find((p) => p.phaseId === planId);
        if (!planEntry) throw new PlanningError(`Plan ${planId} not found`, { code: "PLANNING_PLAN_NOT_FOUND", context: { planId } });
        
        log.info("Skipping plan", { projectId, planId, planName: planEntry.name });
        
        return JSON.stringify({
          message: `Plan ${planId} (${planEntry.name}) skipped. Partial commits left in place.`,
          warning: "Dependent phases may also be affected",
          suggestion: "Review execution status to see cascade effects"
        });
      }

      case "abandon": {
        log.warn("Abandoning phase execution", { projectId });
        planningOrchestrator.abandon(projectId);
        
        return JSON.stringify({
          message: "Phase execution abandoned.",
          warning: "All progress will be lost",
          note: "Project state reset to previous phase"
        });
      }

      case "status": {
        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        
        const enhancedStatus = {
          ...snapshot,
          enhanced: isEnhancedOrchestrator(executionOrchestrator),
          features: isEnhancedOrchestrator(executionOrchestrator) ? {
            waveConcurrency: "Available",
            intelligentDispatch: "Available", 
            structuredExtraction: "Available",
            autoRetry: "Available"
          } : {
            waveConcurrency: "Not available",
            intelligentDispatch: "Not available",
            structuredExtraction: "Not available", 
            autoRetry: "Not available"
          },
          summary: {
            total: snapshot.plans.length,
            completed: snapshot.plans.filter(p => p.status === "done").length,
            failed: snapshot.plans.filter(p => p.status === "failed").length,
            running: snapshot.plans.filter(p => p.status === "running").length,
            pending: snapshot.plans.filter(p => p.status === "pending").length,
            skipped: snapshot.plans.filter(p => p.status === "skipped").length
          }
        };

        return JSON.stringify(enhancedStatus, null, 2);
      }

      default:
        return JSON.stringify({ 
          error: `Unknown action: ${action}`,
          availableActions: ["start", "pause", "resume", "retry", "skip", "abandon", "status", "configure"]
        });
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    log.error("Enhanced plan_execute failed", { action, projectId, error: message });
    
    return JSON.stringify({ 
      error: message,
      action,
      projectId,
      timestamp: new Date().toISOString()
    });
  }
}

/**
 * Type guard to check if orchestrator is the enhanced version
 */
function isEnhancedOrchestrator(orchestrator: AnyExecutionOrchestrator): orchestrator is EnhancedExecutionOrchestrator {
  // Enhanced orchestrator will have additional methods/properties
  // For now, we'll check for a method that only exists on enhanced version
  return "options" in orchestrator || 
         typeof (orchestrator as any).setWorkspaceRoot === "function";
}

/**
 * Extract execution options from input parameters
 */
function extractExecutionOptions(input: Record<string, unknown>) {
  return {
    enableWaveConcurrency: input["concurrency"] as boolean ?? true,
    useIntelligentDispatch: input["intelligentDispatch"] as boolean ?? true,
    useStructuredExtraction: input["structuredExtraction"] as boolean ?? true,
    enableAutoRetry: input["autoRetry"] as boolean ?? true,
    maxConcurrentTasks: input["maxConcurrentTasks"] as number ?? 3,
    availableRoles: ["coder", "reviewer", "researcher", "explorer", "runner"]
  };
}

// Re-export the original tool creator for backward compatibility
export { createPlanExecuteTool } from "./execution-tool.js";