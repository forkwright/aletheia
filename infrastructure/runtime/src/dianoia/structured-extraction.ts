// Structured extraction using instructor-js with Zod schemas and automatic retry
import Instructor, { Mode } from "@instructor-ai/instructor";
import { z } from "zod";
import Anthropic from "@anthropic-ai/sdk";
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
  if (taskLower.match(/\b(review|check|analyze|audit|inspect|validate)\b.*\b(code|pr|diff|changes|file)\b/)) {
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
  if (taskLower.match(/\b(research|lookup|fetch|search|find)\b.*\b(documentation|api|library|package)\b/)) {
    return { type: "research", complexity: "medium", requiresTooling: true, readOnly: true };
  }
  
  // Planning indicators
  if (taskLower.match(/\b(plan|design|architect|decompose|break down|organize)\b/)) {
    return { type: "planning", complexity: "high", requiresTooling: false, readOnly: false };
  }
  
  // Verification indicators
  if (taskLower.match(/\b(verify|confirm|ensure|validate)\b.*\b(complete|goal|requirement|criteria)\b/)) {
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
 * Extract structured data using instructor-js directly with Anthropic models.
 * This bypasses manual JSON extraction and uses instructor's built-in retry mechanism.
 * 
 * @param instructorClient - Pre-configured instructor client
 * @param messages - Array of message objects for the LLM
 * @param schema - Zod schema to validate against
 * @param systemMessage - Optional system message
 * @returns Parsed and validated result, or null if extraction fails
 */
export async function extractStructuredWithInstructor<T>(
  instructorClient: ReturnType<typeof Instructor.from_anthropic>,
  messages: Array<{ role: string; content: string }>,
  schema: z.ZodSchema<T>,
  systemMessage?: string
): Promise<T | null> {
  try {
    const result = await instructorClient.chat.completions.create({
      model: "claude-3-5-sonnet-20241022", // Default model
      messages: messages as any,
      response_model: schema,
      system: systemMessage,
      max_tokens: 4096,
      max_retries: 2, // Built-in retry with error feedback
    });
    
    return result;
  } catch (error) {
    log.error("Instructor extraction failed", { error });
    return null;
  }
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
 * Enhanced dispatch response parser that can use instructor-js when available.
 * Falls back to manual JSON extraction if instructor client is not provided.
 */
export async function parseDispatchResponseWithInstructor(
  responseTextOrMessages: string | Array<{ role: string; content: string }>,
  instructorClient?: ReturnType<typeof Instructor.from_anthropic>,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<DispatchResult | null> {
  if (instructorClient && Array.isArray(responseTextOrMessages)) {
    // Use instructor for structured extraction
    return extractStructuredWithInstructor(
      instructorClient,
      responseTextOrMessages,
      DispatchResultSchema,
      "You are analyzing task dispatch results. Return the structured result as specified."
    );
  } else if (typeof responseTextOrMessages === 'string') {
    // Fall back to manual JSON extraction
    return extractStructured(responseTextOrMessages, DispatchResultSchema, retryCallback);
  } else {
    log.error("Invalid input: expected string when instructor client not available");
    return null;
  }
}

/**
 * Enhanced sub-agent response parser that can use instructor-js when available.
 * Falls back to manual JSON extraction if instructor client is not provided.
 */
export async function parseSubAgentResponseWithInstructor(
  responseTextOrMessages: string | Array<{ role: string; content: string }>,
  instructorClient?: ReturnType<typeof Instructor.from_anthropic>,
  retryCallback?: (errorMessage: string) => Promise<string>
): Promise<SubAgentResult | null> {
  if (instructorClient && Array.isArray(responseTextOrMessages)) {
    // Use instructor for structured extraction
    return extractStructuredWithInstructor(
      instructorClient,
      responseTextOrMessages,
      SubAgentResultSchema,
      "You are a sub-agent providing structured task results. Return your result exactly as specified in the schema."
    );
  } else if (typeof responseTextOrMessages === 'string') {
    // Fall back to manual JSON extraction
    return extractStructured(responseTextOrMessages, SubAgentResultSchema, retryCallback);
  } else {
    log.error("Invalid input: expected string when instructor client not available");
    return null;
  }
}

/**
 * Create an instructor client for structured LLM outputs with Anthropic models.
 * Uses instructor-js with proper Anthropic SDK integration.
 */
export function createInstructorClient(apiKey?: string, oauthToken?: string) {
  if (!apiKey && !oauthToken) {
    log.warn("No Anthropic API key or OAuth token provided for instructor client");
    return null;
  }
  
  try {
    // Create Anthropic client with OAuth or API key
    const anthropicClient = oauthToken ? 
      new Anthropic({
        authToken: oauthToken,
        defaultHeaders: {
          "anthropic-beta": "oauth-2025-04-20"
        }
      }) :
      new Anthropic({
        apiKey: apiKey!
      });
    
    // Create instructor client using from_anthropic
    const instructor = Instructor.from_anthropic(anthropicClient, {
      mode: Mode.ANTHROPIC_TOOLS, // Use tools mode for reliable extraction
    });
    
    log.debug("Instructor client created with Anthropic provider");
    return instructor;
  } catch (error) {
    log.error("Failed to create instructor client", { error });
    return null;
  }
}

/**
 * Create instructor client from environment or credential files.
 * Integrates with existing Aletheia credential system.
 */
export async function createInstructorClientFromCredentials() {
  // Try environment variables first
  const envApiKey = process.env.ANTHROPIC_API_KEY;
  if (envApiKey) {
    return createInstructorClient(envApiKey);
  }
  
  // Try credential files - this would need to import the credential loading logic
  // from hermeneus/router.ts, but for now we'll keep it simple
  log.debug("No environment API key found, instructor client not created");
  return null;
}

// Export schemas for testing and external use
export const schemas = {
  SubAgentResult: SubAgentResultSchema,
  TaskExecutionResult: TaskExecutionResultSchema,
  DispatchResult: DispatchResultSchema,
};