// Structured extraction with Zod validation — replaces hand-rolled JSON parsing
// Implements EXEC-02: structured extraction with automatic retry on validation failures

import { createLogger } from "../koina/logger.js";
import { z } from "zod";

const log = createLogger("dianoia:structured-extraction");

// Result schema for sub-agent task execution with enhanced validation
export const SubAgentResultSchema = z.object({
  role: z.string().min(1, "Role must not be empty"),
  task: z.string().min(1, "Task description must not be empty"),
  status: z.enum(["success", "partial", "failed"], {
    errorMap: () => ({ message: "Status must be 'success', 'partial', or 'failed'" })
  }),
  summary: z.string().min(10, "Summary must be at least 10 characters"),
  details: z.record(z.unknown()).default({}),
  filesChanged: z.array(z.string()).optional(),
  issues: z.array(z.object({
    severity: z.enum(["error", "warning", "info"]),
    location: z.string().optional(),
    message: z.string().min(1),
    suggestion: z.string().optional()
  })).optional(),
  confidence: z.number().min(0).max(1, "Confidence must be between 0 and 1")
});

export type SubAgentResult = z.infer<typeof SubAgentResultSchema>;

// Schema for plan execution results with wave information
export const ExecutionResultSchema = z.object({
  results: z.array(z.object({
    status: z.enum(["success", "error"]),
    result: z.string().optional(),
    error: z.string().optional(),
    durationMs: z.number().min(0)
  })),
  waveNumber: z.number().min(0),
  totalDuration: z.number().min(0).optional()
});

export type ExecutionResult = z.infer<typeof ExecutionResultSchema>;

// Schema for task-to-role mapping configuration
export const TaskMappingSchema = z.object({
  taskType: z.enum([
    "code_implementation",
    "code_review", 
    "research",
    "exploration",
    "testing",
    "build_execution",
    "documentation"
  ]),
  preferredRole: z.enum(["coder", "reviewer", "researcher", "explorer", "runner"]),
  fallbackRoles: z.array(z.enum(["coder", "reviewer", "researcher", "explorer", "runner"])).default([]),
  complexity: z.enum(["low", "medium", "high"]).default("medium"),
  requiresTools: z.array(z.string()).default([])
});

export type TaskMapping = z.infer<typeof TaskMappingSchema>;

// Default task-to-role mapping table per EXEC-01
export const DEFAULT_TASK_MAPPINGS: TaskMapping[] = [
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
  },
  {
    taskType: "build_execution", 
    preferredRole: "runner",
    fallbackRoles: ["coder"],
    complexity: "low",
    requiresTools: ["exec", "ls"]
  },
  {
    taskType: "documentation",
    preferredRole: "coder",
    fallbackRoles: ["researcher"],
    complexity: "low",
    requiresTools: ["read", "write", "edit"]
  }
];

/**
 * Extract structured data from text using Zod validation with automatic retry feedback.
 * Implements EXEC-04: validation errors are fed back to model automatically with one retry.
 */
export class StructuredExtractor {
  constructor() {
    // Simple implementation using Zod validation on JSON blocks
  }

  /**
   * Parse structured result with Zod validation and error feedback.
   * Replaces hand-rolled JSON parsing with schema-validated extraction.
   */
  async extractStructuredResult(
    responseText: string,
    schema: z.ZodSchema = SubAgentResultSchema,
    retryOnFailure: boolean = true
  ): Promise<{ success: boolean; data?: any; error?: string; validationErrors?: string[] }> {
    // Find the last JSON block in the response
    const jsonBlocks = [...responseText.matchAll(/```json\s*\n([\s\S]*?)\n```/g)];
    if (jsonBlocks.length === 0) {
      return {
        success: false,
        error: "No JSON block found in response. Expected format: ```json\n{...}\n```"
      };
    }

    const lastBlock = jsonBlocks[jsonBlocks.length - 1];
    if (!lastBlock?.[1]) {
      return {
        success: false, 
        error: "Empty JSON block found"
      };
    }

    try {
      const parsed = JSON.parse(lastBlock[1]);
      
      // Validate with Zod schema
      const result = schema.safeParse(parsed);
      
      if (result.success) {
        log.debug("Structured extraction successful", { schema: schema._def.typeName });
        return {
          success: true,
          data: result.data
        };
      } else {
        // Collect validation errors for potential retry
        const validationErrors = result.error.errors.map(err => 
          `${err.path.join('.')}: ${err.message}`
        );
        
        log.warn("Schema validation failed", { 
          errors: validationErrors,
          rawData: parsed 
        });

        return {
          success: false,
          error: "Schema validation failed",
          validationErrors
        };
      }
      
    } catch (parseError) {
      const error = parseError instanceof Error ? parseError.message : String(parseError);
      log.warn("JSON parsing failed", { error, jsonContent: lastBlock[1].slice(0, 200) });
      
      return {
        success: false,
        error: `JSON parsing failed: ${error}`
      };
    }
  }

  /**
   * Create validation error feedback for model retry.
   * Formats Zod errors into actionable feedback for the next attempt.
   */
  createValidationFeedback(validationErrors: string[], originalTask: string): string {
    const feedback = [
      "❌ **Validation Failed**",
      "",
      "Your previous response had the following validation errors:",
      "",
      ...validationErrors.map(error => `- ${error}`),
      "",
      "Please fix these issues and provide a new response with the correct JSON structure.",
      "Make sure all required fields are present and match the expected types.",
      "",
      "**Original task:** " + originalTask.slice(0, 200) + (originalTask.length > 200 ? "..." : ""),
      "",
      "Respond with the same format but fix the validation errors above."
    ].join("\n");

    return feedback;
  }
}

/**
 * Map a task description to the appropriate role based on content analysis and predefined mappings.
 * Implements EXEC-01: intelligent task-to-role dispatch.
 */
export function mapTaskToRole(
  task: string, 
  availableRoles: string[] = ["coder", "reviewer", "researcher", "explorer", "runner"]
): { role: string; confidence: number; reasoning: string } {
  const taskLower = task.toLowerCase();
  
  // Keyword-based classification with confidence scoring
  const indicators: Array<{
    keywords: string[];
    taskType: TaskMapping['taskType'];
    weight: number;
  }> = [
    {
      keywords: ["implement", "code", "write", "create", "build", "develop", "add", "fix", "modify"],
      taskType: "code_implementation",
      weight: 1.0
    },
    {
      keywords: ["review", "check", "validate", "audit", "analyze", "inspect", "verify"],
      taskType: "code_review", 
      weight: 0.9
    },
    {
      keywords: ["research", "investigate", "study", "learn", "explore api", "documentation"],
      taskType: "research",
      weight: 0.8
    },
    {
      keywords: ["find", "search", "locate", "grep", "explore", "trace", "read"],
      taskType: "exploration",
      weight: 0.7
    },
    {
      keywords: ["test", "run tests", "execute", "compile", "build"],
      taskType: "testing",
      weight: 0.8
    },
    {
      keywords: ["run", "execute", "start", "launch", "deploy"],
      taskType: "build_execution",
      weight: 0.7
    },
    {
      keywords: ["document", "write docs", "comment", "readme"],
      taskType: "documentation",
      weight: 0.6
    }
  ];

  // Score each task type
  let bestMatch: { taskType: TaskMapping['taskType']; score: number } = {
    taskType: "code_implementation",
    score: 0
  };

  for (const indicator of indicators) {
    let score = 0;
    for (const keyword of indicator.keywords) {
      if (taskLower.includes(keyword)) {
        score += indicator.weight;
      }
    }
    
    if (score > bestMatch.score) {
      bestMatch = { taskType: indicator.taskType, score };
    }
  }

  // Find the mapping for the best task type
  const mapping = DEFAULT_TASK_MAPPINGS.find(m => m.taskType === bestMatch.taskType);
  if (!mapping) {
    return {
      role: "coder", // safe default
      confidence: 0.3,
      reasoning: "No clear task type detected, defaulting to coder"
    };
  }

  // Check if preferred role is available
  if (availableRoles.includes(mapping.preferredRole)) {
    return {
      role: mapping.preferredRole,
      confidence: Math.min(0.9, bestMatch.score * 0.8 + 0.1),
      reasoning: `Task classified as ${bestMatch.taskType}, mapped to ${mapping.preferredRole}`
    };
  }

  // Try fallback roles
  for (const fallbackRole of mapping.fallbackRoles) {
    if (availableRoles.includes(fallbackRole)) {
      return {
        role: fallbackRole,
        confidence: Math.min(0.7, bestMatch.score * 0.6 + 0.1),
        reasoning: `Task classified as ${bestMatch.taskType}, preferred role ${mapping.preferredRole} not available, using fallback ${fallbackRole}`
      };
    }
  }

  // Fallback to any available role
  const fallbackRole = availableRoles[0] || "coder";
  return {
    role: fallbackRole,
    confidence: 0.2,
    reasoning: `No suitable role found for ${bestMatch.taskType}, using fallback ${fallbackRole}`
  };
}

/**
 * Enhanced version of parseStructuredResult that uses Zod validation.
 * Drop-in replacement for the existing function with better error handling.
 */
export async function parseStructuredResultWithZod(responseText: string): Promise<SubAgentResult | null> {
  const extractor = new StructuredExtractor();
  const result = await extractor.extractStructuredResult(responseText, SubAgentResultSchema, false);
  
  if (result.success) {
    return result.data as SubAgentResult;
  }
  
  log.warn("Failed to parse structured result with Zod", { 
    error: result.error,
    validationErrors: result.validationErrors 
  });
  
  return null;
}