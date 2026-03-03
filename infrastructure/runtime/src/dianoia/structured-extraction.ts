// Structured extraction using instructor-js with Zod schemas and automatic retry
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
  if (taskLower.match(/\b(review|check|analyze|audit|inspect|validate)\b.*\b(code|pr|pull request|diff|changes|file|implementation|module|component)\b/)) {
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
  
  // Research indicators - fixed to match the test
  if (taskLower.match(/\b(research|lookup|fetch)\b/)) {
    return { type: "research", complexity: "medium", requiresTooling: true, readOnly: true };
  }
  
  // Planning indicators
  if (taskLower.match(/\b(plan|design|architect|decompose|break down|organize)\b/)) {
    return { type: "planning", complexity: "high", requiresTooling: false, readOnly: false };
  }
  
  // Verification indicators - fixed to match the test
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

// Achievement claim from sub-agent completion assertions (EXEC-02)
const AchievementSchema = z.object({
  claim: z.string().min(1),
  evidence: z.string().optional(),
  verifiable: z.boolean().optional(),
});

export type Achievement = z.infer<typeof AchievementSchema>;

// Sub-agent result schema matching the existing interface
const SubAgentResultSchema = z.object({
  role: z.string().min(1, "Role must not be empty"),
  task: z.string().min(1, "Task must not be empty"),
  status: z.enum(["success", "partial", "failed"]),
  summary: z.string().min(1, "Summary must not be empty"),
  details: z.record(z.unknown()),
  filesChanged: z.array(z.string()).optional(),
  issues: z.array(z.object({
    severity: z.enum(["error", "warning", "info"]),
    location: z.string().optional(),
    message: z.string(),
    suggestion: z.string().optional(),
  })).optional(),
  confidence: z.number().min(0).max(1),
  achievements: z.array(AchievementSchema).optional(),
  blockers: z.array(z.string()).optional(),
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
    // First: try direct JSON parse (dispatch tool returns raw JSON, not fenced)
    const trimmed = responseText.trim();
    if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
      try {
        const directParsed = JSON.parse(trimmed);
        const directResult = schema.parse(directParsed);
        return directResult;
      } catch {
        // Fall through to extraction strategies
      }
    }

    // Second: try to extract JSON block from markdown/prose response
    const jsonBlock = extractJsonBlock(responseText);
    if (!jsonBlock) {
      const errorMsg = "No JSON found in response (tried direct parse and extraction from prose).";
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
 * Extract JSON from response text using multiple strategies (most specific → most forgiving).
 * 
 * Strategies in order:
 * 1. Fenced ```json ... ``` blocks (preferred)
 * 2. Fenced ``` ... ``` blocks that parse as JSON
 * 3. Raw JSON object at end of response (after last prose paragraph)
 * 4. First { ... } block that parses as valid JSON
 */
function extractJsonBlock(responseText: string): string | null {
  // Strategy 1: Fenced json blocks
  const jsonBlocks = [...responseText.matchAll(/```json\s*\n([\s\S]*?)\n```/g)];
  if (jsonBlocks.length > 0) {
    const lastBlock = jsonBlocks[jsonBlocks.length - 1];
    if (lastBlock?.[1]?.trim()) return lastBlock[1].trim();
  }

  // Strategy 2: Any fenced block that parses as JSON
  const anyFenced = [...responseText.matchAll(/```\w*\s*\n([\s\S]*?)\n```/g)];
  for (let i = anyFenced.length - 1; i >= 0; i--) {
    const candidate = anyFenced[i]?.[1]?.trim();
    if (candidate && candidate.startsWith("{")) {
      try { JSON.parse(candidate); return candidate; } catch { /* not json */ }
    }
  }

  // Strategy 3: Look for JSON object at the end of the response
  const trimmed = responseText.trim();
  const lastBrace = trimmed.lastIndexOf("}");
  if (lastBrace > 0) {
    // Walk backwards from last } to find matching {
    let depth = 0;
    let inString = false;
    let escape = false;
    for (let i = lastBrace; i >= 0; i--) {
      const ch = trimmed[i]!;
      if (escape) { escape = false; continue; }
      if (ch === "\\") { escape = true; continue; }
      if (ch === '"') { inString = !inString; continue; }
      if (inString) continue;
      if (ch === "}") depth++;
      if (ch === "{") { depth--; if (depth === 0) {
        const candidate = trimmed.slice(i, lastBrace + 1);
        try { JSON.parse(candidate); return candidate; } catch { /* not valid */ }
      }}
    }
  }

  return null;
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
export function parseDispatchResponse(
  responseText: string,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<DispatchResult | null> {
  return extractStructured(responseText, DispatchResultSchema, retryCallback);
}

/**
 * Parse sub-agent response using structured extraction with retry.
 * Replaces parseStructuredResult in roles/index.ts
 */
export function parseSubAgentResponse(
  responseText: string,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<SubAgentResult | null> {
  return extractStructured(responseText, SubAgentResultSchema, retryCallback);
}

// --- Rich role mapping with confidence and reasoning ---

export interface RoleMapping {
  role: "coder" | "reviewer" | "researcher" | "explorer" | "runner";
  confidence: number;
  reasoning: string;
}

/**
 * Map a task description to an appropriate role with confidence score and reasoning.
 * Richer API than selectRoleForTask — includes why the mapping was chosen and
 * how confident we are. Supports optional role constraints for fallback scenarios.
 */
export function mapTaskToRole(task: string, availableRoles?: string[]): RoleMapping {
  const classification = classifyTask(task);
  const preferredRole = taskTypeToRole(classification);

  // If no constraints or preferred role is available, use it directly
  if (!availableRoles || availableRoles.includes(preferredRole)) {
    return {
      role: preferredRole,
      confidence: classification.complexity === "low" ? 0.95 : classification.complexity === "medium" ? 0.85 : 0.75,
      reasoning: `Task classified as ${classification.type} (${classification.complexity} complexity) → ${preferredRole}`,
    };
  }

  // Fallback: pick best available role
  const fallbackOrder: Array<"coder" | "reviewer" | "researcher" | "explorer" | "runner"> = [
    "coder", "reviewer", "researcher", "explorer", "runner",
  ];
  const fallbackRole = fallbackOrder.find((r) => availableRoles.includes(r)) ?? "coder";

  return {
    role: fallbackRole as RoleMapping["role"],
    confidence: 0.5,
    reasoning: `Preferred ${preferredRole} unavailable; fallback to ${fallbackRole} from available roles [${availableRoles.join(", ")}]`,
  };
}

// --- StructuredExtractor class API ---

export interface ExtractionResult<T = unknown> {
  success: boolean;
  data: T;
  validationErrors?: string[];
  rawJson?: string;
}

/**
 * Class-based structured extraction for integration with execution engine.
 * Wraps extractStructured() and extractJsonBlock() with validation feedback generation.
 */
export class StructuredExtractor {
  /**
   * Extract and validate structured data from a response string.
   * Returns success/failure with typed data and validation errors.
   */
  async extractStructuredResult<T>(
    responseText: string,
    schema: z.ZodSchema<T>,
    retryCallback?: (errorMessage: string) => Promise<string>,
  ): Promise<ExtractionResult<T>> {
    // Try extraction
    const jsonText = extractJsonBlock(responseText);
    if (!jsonText) {
      return {
        success: false,
        data: undefined as unknown as T,
        validationErrors: ["No JSON found in response text"],
      };
    }

    try {
      const parsed = JSON.parse(jsonText);
      const validated = schema.parse(parsed);
      return { success: true, data: validated, rawJson: jsonText };
    } catch (error) {
      if (error instanceof z.ZodError) {
        const errors = error.issues.map((issue) => {
          const path = issue.path.length > 0 ? `${issue.path.join(".")}: ` : "";
          return `${path}${issue.message}`;
        });

        // Attempt retry if callback provided
        if (retryCallback) {
          const feedback = this.createValidationFeedback(errors, "retry");
          try {
            const retryText = await retryCallback(feedback);
            const retryJson = extractJsonBlock(retryText);
            if (retryJson) {
              const retryParsed = JSON.parse(retryJson);
              const retryValidated = schema.parse(retryParsed);
              return { success: true, data: retryValidated, rawJson: retryJson };
            }
          } catch {
            // Retry also failed
          }
        }

        return {
          success: false,
          data: undefined as unknown as T,
          validationErrors: errors,
          rawJson: jsonText,
        };
      }

      return {
        success: false,
        data: undefined as unknown as T,
        validationErrors: [error instanceof Error ? error.message : String(error)],
        rawJson: jsonText,
      };
    }
  }

  /**
   * Create actionable validation feedback for retry prompts.
   */
  createValidationFeedback(errors: string[], originalTask: string): string {
    const lines = [
      "❌ **Validation Failed**",
      "",
      "Your response had the following validation errors:",
      "",
      ...errors.map((e) => `- ${e}`),
      "",
      `Please fix these issues and respond with valid JSON for: ${originalTask}`,
    ];
    return lines.join("\n");
  }
}

// Export schemas for testing and external use
export const schemas = {
  SubAgentResult: SubAgentResultSchema,
  TaskExecutionResult: TaskExecutionResultSchema,
  DispatchResult: DispatchResultSchema,
};