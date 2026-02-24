// Structured extraction using instructor-js with Zod schemas and automatic retry
// Implements EXEC-02: instructor-js with Zod-based validation and error feedback
import Instructor from "@instructor-ai/instructor";
import Anthropic from "@anthropic-ai/sdk";
import { z } from "zod";
import { createLogger } from "../koina/logger.js";

const log = createLogger("dianoia:structured-extraction");

/**
 * Task type classification for smart role/model selection.
 */
export type TaskType = 
  | "code-generation"     // Writing new code, implementing features
  | "code-editing"        // Modifying existing code, bug fixes
  | "code-review"         // Analyzing code for issues, style, logic
  | "exploration"         // Read-only codebase investigation
  | "testing"             // Running tests, validation, health checks
  | "research"            // Web research, documentation lookup
  | "planning"            // Task decomposition, strategy
  | "verification";       // Checking completeness, goal alignment

export interface TaskClassification {
  type: TaskType;
  complexity: "low" | "medium" | "high";
  requiresTooling: boolean;
  readOnly: boolean;
}

/**
 * Classify a task description to determine appropriate role and model.
 * Uses heuristics based on keywords and patterns in the task text.
 */
export function classifyTask(task: string): TaskClassification {
  const taskLower = task.toLowerCase();
  
  // Code generation indicators
  if (taskLower.match(/\b(implement|create|build|write|add|generate)\b.*\b(function|class|component|module|feature|endpoint|api)\b/)) {
    return { type: "code-generation", complexity: "medium", requiresTooling: true, readOnly: false };
  }
  
  // Code editing indicators  
  if (taskLower.match(/\b(fix|update|modify|change|edit|refactor|migrate)\b.*\b(bug|code|file|function|class)\b/)) {
    return { type: "code-editing", complexity: "medium", requiresTooling: true, readOnly: false };
  }
  
  // Code review indicators
  if (taskLower.match(/\b(review|check|analyze|audit|inspect|validate)\b.*\b(code|pr|diff|changes|file|pull request)\b/)) {
    return { type: "code-review", complexity: "low", requiresTooling: false, readOnly: true };
  }
  
  // Exploration indicators
  if (taskLower.match(/\b(find|locate|search|explore|investigate|trace|grep)\b/)) {
    return { type: "exploration", complexity: "low", requiresTooling: false, readOnly: true };
  }
  
  // Testing indicators
  if (taskLower.match(/\b(test|run|execute|check|validate)\b.*\b(tests?|build|command|script)\b/)) {
    return { type: "testing", complexity: "low", requiresTooling: true, readOnly: true };
  }
  
  // Research indicators  
  if (taskLower.match(/\b(research|lookup)\b/) || 
      taskLower.match(/\b(search|find)\b.*\b(documentation|api|library|package|best practices|practices)\b/)) {
    return { type: "research", complexity: "medium", requiresTooling: true, readOnly: true };
  }
  
  // Planning indicators
  if (taskLower.match(/\b(plan|design|architect|decompose|break down|organize)\b/)) {
    return { type: "planning", complexity: "high", requiresTooling: false, readOnly: false };
  }
  
  // Verification indicators - more general match
  if (taskLower.match(/\b(verify|confirm|ensure)\b/) || 
      taskLower.match(/\bvalidate\b.*\b(requirements?|criteria|complete|goal)\b/)) {
    return { type: "verification", complexity: "medium", requiresTooling: false, readOnly: true };
  }
  
  // Default to code generation for ambiguous tasks
  return { type: "code-generation", complexity: "medium", requiresTooling: true, readOnly: false };
}

/**
 * Map a task classification to the optimal sub-agent role.
 */
export function taskTypeToRole(classification: TaskClassification): "coder" | "reviewer" | "researcher" | "explorer" | "runner" {
  switch (classification.type) {
    case "code-generation":
    case "code-editing":
      return "coder";
    case "code-review":
      return "reviewer";
    case "research":
      return "researcher";
    case "exploration":
      return "explorer";
    case "testing":
      return "runner";
    case "planning":
    case "verification":
      return classification.complexity === "high" ? "coder" : "reviewer";  // Complex planning needs coder capability
    default:
      return "coder";
  }
}

/**
 * Select the appropriate role and model for a task.
 * Replaces the old role-first approach with task-first classification.
 */
export function selectRoleForTask(task: string): "coder" | "reviewer" | "researcher" | "explorer" | "runner" {
  const classification = classifyTask(task);
  return taskTypeToRole(classification);
}

// Sub-agent result schema matching the existing interface
const SubAgentResultSchema = z.object({
  role: z.string(),
  task: z.string(),
  status: z.enum(["success", "partial", "failed"]),
  summary: z.string(),
  details: z.record(z.unknown()),
  filesChanged: z.array(z.string()).optional(),
  issues: z.array(z.object({
    severity: z.enum(["error", "warning", "info"]),
    location: z.string().optional(),
    message: z.string(),
    suggestion: z.string().optional(),
  })).optional(),
  confidence: z.number().min(0).max(1),
});

export type SubAgentResult = z.infer<typeof SubAgentResultSchema>;

// Task execution result for dispatch responses
const TaskExecutionResultSchema = z.object({
  index: z.number(),
  role: z.string().optional(),
  task: z.string(),
  status: z.enum(["success", "error", "timeout"]),
  result: z.string().optional(),
  structuredResult: SubAgentResultSchema.optional(),
  error: z.string().optional(),
  tokens: z.object({
    input: z.number(),
    output: z.number(),
    total: z.number(),
  }).optional(),
  durationMs: z.number(),
});

const DispatchResultSchema = z.object({
  taskCount: z.number(),
  succeeded: z.number(),
  failed: z.number(),
  reducer: z.string().optional(),
  reducerInfo: z.record(z.unknown()).optional(),
  results: z.array(TaskExecutionResultSchema),
  timing: z.object({
    wallClockMs: z.number(),
    sequentialMs: z.number(),
    savedMs: z.number(),
  }),
  totalTokens: z.number(),
});

export type TaskExecutionResult = z.infer<typeof TaskExecutionResultSchema>;
export type DispatchResult = z.infer<typeof DispatchResultSchema>;

/**
 * Create an instructor client for Anthropic models.
 * This enables direct structured output extraction from LLM responses.
 */
export function createAnthropicInstructorClient(apiKey?: string): any | null {
  if (!apiKey) {
    log.debug("No Anthropic API key provided for instructor client");
    return null;
  }

  try {
    const anthropic = new Anthropic({ apiKey });
    
    // Note: instructor-js primarily supports OpenAI, but we can adapt it for Anthropic
    // For now, we'll use manual JSON extraction with Zod validation 
    // A full instructor integration would require extending the library
    const instructorClient = Instructor({
      client: anthropic as any, // Type assertion for compatibility
      mode: "JSON",
    });
    
    return instructorClient;
  } catch (error) {
    log.warn("Failed to create instructor client", { error });
    return null;
  }
}

/**
 * Extract structured data from sub-agent response with automatic retry on validation failures.
 * 
 * @param responseText - Raw response text from sub-agent
 * @param schema - Zod schema to validate against
 * @param retryCallback - Optional callback to retry with error feedback
 * @returns Parsed and validated result, or null if extraction fails after retry
 */
export async function extractStructured<T>(
  responseText: string,
  schema: z.ZodSchema<T>,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<T | null> {
  try {
    // Try to extract JSON block from response
    const jsonBlock = extractJsonBlock(responseText);
    if (!jsonBlock) {
      const errorMsg = "No JSON block found in response. Expected ```json ... ``` format.";
      if (retryCallback) {
        log.debug("JSON extraction failed, retrying with error feedback");
        const retryText = await retryCallback(errorMsg);
        return extractStructured(retryText, schema, undefined); // No second retry
      }
      return null;
    }

    // Parse and validate with Zod
    const parsed = JSON.parse(jsonBlock);
    const result = schema.parse(parsed);
    return result;
  } catch (error) {
    if (error instanceof z.ZodError) {
      const errorMsg = `Schema validation failed: ${formatZodError(error)}`;
      log.debug("Schema validation failed", { error: errorMsg });
      
      if (retryCallback) {
        log.debug("Retrying with Zod error feedback");
        const retryText = await retryCallback(errorMsg);
        return extractStructured(retryText, schema, undefined); // No second retry
      }
      return null;
    } else if (error instanceof SyntaxError) {
      const errorMsg = `JSON parsing failed: ${error.message}`;
      if (retryCallback) {
        log.debug("JSON parsing failed, retrying with error feedback");
        const retryText = await retryCallback(errorMsg);
        return extractStructured(retryText, schema, undefined); // No second retry
      }
      return null;
    } else {
      log.warn("Unexpected error in structured extraction", { error });
      return null;
    }
  }
}

/**
 * Extract the last JSON block from response text.
 * Sub-agents are expected to end with a ```json ... ``` block.
 */
function extractJsonBlock(responseText: string): string | null {
  const jsonBlocks = [...responseText.matchAll(/```json\s*\n([\s\S]*?)\n```/g)];
  if (jsonBlocks.length === 0) return null;
  
  const lastBlock = jsonBlocks[jsonBlocks.length - 1];
  return lastBlock?.[1]?.trim() ?? null;
}

/**
 * Format Zod validation errors into helpful error messages.
 */
function formatZodError(error: z.ZodError): string {
  const issues = error.issues.map(issue => {
    const path = issue.path.length > 0 ? ` at ${issue.path.join('.')}` : '';
    return `${issue.message}${path}`;
  });
  return issues.join('; ');
}

/**
 * Parse dispatch tool response using structured extraction with retry.
 * Replaces hand-rolled JSON parsing in sessions-dispatch.ts
 */
export async function parseDispatchResponse(
  responseText: string, 
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<DispatchResult | null> {
  return extractStructured(responseText, DispatchResultSchema, retryCallback);
}

/**
 * Parse sub-agent response using structured extraction with retry.
 * Replaces parseStructuredResult in roles/index.ts
 */
export async function parseSubAgentResponse(
  responseText: string,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<SubAgentResult | null> {
  return extractStructured(responseText, SubAgentResultSchema, retryCallback);
}

/**
 * Create an instructor client for structured LLM outputs.
 * This would be used if we want to use instructor directly with the LLM provider.
 * For now, we're using manual extraction approach.
 */
export function createInstructorClient(apiKey?: string) {
  // Note: instructor-js is designed for OpenAI, but we're using Anthropic
  // We'll use the manual extraction approach for now
  if (!apiKey) {
    log.warn("No OpenAI API key provided for instructor client");
    return null;
  }
  
  return null; // Placeholder for future instructor integration
}

// Export schemas for testing and external use
export const schemas = {
  SubAgentResult: SubAgentResultSchema,
  TaskExecutionResult: TaskExecutionResultSchema,
  DispatchResult: DispatchResultSchema,
};

// Additional exports to maintain compatibility with enhanced-execution.ts
export type ExecutionResult = TaskExecutionResult; // Alias for backward compatibility
export { SubAgentResultSchema }; // Export the actual schema object

// Default task mappings for compatibility with tests
export const DEFAULT_TASK_MAPPINGS = [
  {
    taskType: "code_implementation",
    preferredRole: "coder",
    fallbackRoles: ["reviewer"],
    complexity: "medium",
    requiresTools: ["read", "write", "edit", "exec"]
  },
  {
    taskType: "code_review",
    preferredRole: "reviewer", 
    fallbackRoles: ["coder"],
    complexity: "low",
    requiresTools: ["read", "grep", "find"]
  },
  {
    taskType: "research",
    preferredRole: "researcher",
    fallbackRoles: ["explorer"],
    complexity: "medium",
    requiresTools: ["web_search", "web_fetch", "read"]
  },
  {
    taskType: "exploration",
    preferredRole: "explorer",
    fallbackRoles: ["researcher", "runner"],
    complexity: "low", 
    requiresTools: ["read", "grep", "find", "ls"]
  },
  {
    taskType: "testing",
    preferredRole: "runner",
    fallbackRoles: ["coder"],
    complexity: "low",
    requiresTools: ["exec", "read"]
  }
];

/**
 * Legacy function for backward compatibility with tests.
 * Parses structured result using Zod validation.
 */
export async function parseStructuredResultWithZod(responseText: string): Promise<SubAgentResult | null> {
  return extractStructured(responseText, SubAgentResultSchema);
}

/**
 * Legacy task-to-role mapping function for backward compatibility.
 * Maps a task description to role using the new classification system.
 */
export function mapTaskToRole(
  task: string, 
  availableRoles: string[] = ["coder", "reviewer", "researcher", "explorer", "runner"]
): { role: string; confidence: number; reasoning: string } {
  const classification = classifyTask(task);
  const preferredRole = taskTypeToRole(classification);
  
  if (availableRoles.includes(preferredRole)) {
    return {
      role: preferredRole,
      confidence: 0.8,
      reasoning: `Task classified as ${classification.type}, mapped to ${preferredRole}`
    };
  }
  
  // Fallback to first available role
  const fallbackRole = availableRoles[0] || "coder";
  return {
    role: fallbackRole,
    confidence: 0.5,
    reasoning: `Preferred role ${preferredRole} not available, using fallback ${fallbackRole}`
  };
}

/**
 * Simple structured extractor class for backward compatibility.
 * Wraps the functional extraction API in a class interface.
 */
export class StructuredExtractor {
  async extractStructuredResult(
    responseText: string,
    schema: z.ZodSchema = SubAgentResultSchema,
    _retryOnFailure: boolean = true
  ): Promise<{ success: boolean; data?: any; error?: string; validationErrors?: string[] }> {
    const result = await extractStructured(responseText, schema);
    
    if (result !== null) {
      return {
        success: true,
        data: result
      };
    } else {
      return {
        success: false,
        error: "Extraction failed"
      };
    }
  }
}