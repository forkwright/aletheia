// Canonical error code registry — every error code in the system is defined here
// Format: MODULE_CONDITION (UPPER_SNAKE_CASE)
// AI agents: search this file to understand what errors a module can produce.
// INVARIANT: all codes UPPER_SNAKE_CASE, unique descriptions, module prefix coverage — see invariants.test.ts

export const ERROR_CODES = {
  // hermeneus (provider routing)
  PROVIDER_TIMEOUT: "API call timed out",
  PROVIDER_RATE_LIMITED: "Rate limited by provider",
  PROVIDER_AUTH_FAILED: "Authentication failed",
  PROVIDER_TOKEN_EXPIRED: "OAuth token expired — falling through to backup credential",
  PROVIDER_INVALID_RESPONSE: "Provider returned unexpected response format",
  PROVIDER_NOT_FOUND: "No provider registered for requested model",
  PROVIDER_OVERLOADED: "Provider returned 529 overloaded",

  // mneme (session store)
  SESSION_NOT_FOUND: "Session ID does not exist in store",
  SESSION_LOCKED: "Session is being written to by another process",
  SESSION_CORRUPTED: "Session data failed integrity check",
  STORE_INIT_FAILED: "SQLite database initialization failed",

  // nous (agent management + pipeline)
  AGENT_NOT_FOUND: "Agent ID not in configuration",
  BOOTSTRAP_FAILED: "Failed to assemble agent bootstrap context",
  BOOTSTRAP_OVER_BUDGET: "Bootstrap exceeds token budget",
  TURN_TIMEOUT: "Agent turn exceeded time limit",
  TURN_REJECTED: "Turn rejected by runtime (draining or depth limit)",
  PIPELINE_STAGE_FAILED: "A pipeline stage failed during execution",
  PIPELINE_NO_OUTCOME: "Pipeline completed without producing an outcome",
  PIPELINE_TOOL_LOOP: "Tool call loop detected",
  PIPELINE_MAX_LOOPS: "Turn exceeded maximum tool loop count",
  PIPELINE_WALL_CLOCK: "Turn exceeded wall-clock time limit",
  PIPELINE_STREAM_INCOMPLETE: "Streaming pipeline ended without message_complete",
  PIPELINE_RECALL_FAILED: "Memory recall HTTP request failed",
  TOOL_EXECUTION_FAILED: "Tool call returned an error",
  TOOL_NOT_FOUND: "Agent requested a tool that is not registered",
  EPHEMERAL_LIMIT: "Maximum concurrent ephemeral agents reached",

  // organon (tools)
  EXEC_TIMEOUT: "Shell command timed out",
  EXEC_SIGNAL: "Shell command killed by signal",
  FILE_NOT_FOUND: "Requested file does not exist",
  FILE_PERMISSION_DENIED: "Insufficient permissions for file operation",
  SEARCH_FAILED: "Web/memory search returned an error",
  BROWSER_LAUNCH_FAILED: "Could not launch browser instance",
  BROWSER_MAX_PAGES: "Maximum concurrent browser pages reached",
  PATH_OUTSIDE_WORKSPACE: "Path is outside the agent workspace",
  TOOL_SSRF_BLOCKED: "SSRF protection blocked the requested URL",

  // semeion (Signal)
  SIGNAL_DAEMON_DOWN: "Signal CLI daemon is not running",
  SIGNAL_SEND_FAILED: "Failed to send Signal message",
  SIGNAL_PARSE_ERROR: "Could not parse inbound Signal message",
  SIGNAL_SSE_FAILED: "SSE connection to Signal daemon failed",
  SIGNAL_RPC_ERROR: "Signal RPC call returned an error",

  // semeion (TTS)
  TRANSPORT_TTS_NO_ENGINE: "No TTS engine available — set OPENAI_API_KEY or install Piper",
  TRANSPORT_TTS_HTTP_ERROR: "OpenAI TTS HTTP request failed",
  TRANSPORT_TTS_BINARY_NOT_FOUND: "Piper binary not found at configured path",
  TRANSPORT_TTS_MODEL_NOT_FOUND: "Piper model not found at configured path",

  // taxis (config)
  CONFIG_NOT_FOUND: "Configuration file not found at expected path",
  CONFIG_VALIDATION_FAILED: "Configuration failed schema validation",
  CONFIG_MISSING_REQUIRED: "Required configuration field is missing",
  CONFIG_ANCHOR_NOT_FOUND: "Bootstrap anchor not found — run 'aletheia init' to configure",
  CONFIG_INSTANCE_INVALID: "Instance directory structure validation failed",
  CONFIG_SECRET_UNRESOLVED: "SecretRef could not be resolved — env var not set, file not readable, or file is empty",
  CONFIG_SECRET_VAULT_UNSUPPORTED: "Vault SecretRef source is not yet implemented — a plugin interface is planned for future versions",

  // taxis (scaffold)
  CONFIG_SCAFFOLD_INVALID_ID: "Agent scaffold failed — invalid agent ID",
  CONFIG_SCAFFOLD_EXISTS: "Agent workspace already exists at target path",
  CONFIG_SCAFFOLD_NO_CONFIG: "Cannot read config file for scaffold operation",
  CONFIG_SCAFFOLD_DUPLICATE_ID: "Agent ID already exists in configuration",

  // distillation
  DISTILL_EXTRACTION_FAILED: "Fact extraction returned no results",
  DISTILL_SUMMARY_FAILED: "Summary generation failed",
  DISTILL_INSUFFICIENT_MESSAGES: "Not enough messages to distill",

  // prostheke (plugins)
  PLUGIN_LOAD_FAILED: "Plugin failed to load",
  PLUGIN_HOOK_ERROR: "Plugin hook threw an error",

  // pylon (gateway)
  GATEWAY_AUTH_FAILED: "Request authentication failed",
  GATEWAY_INVALID_REQUEST: "Request body failed validation",

  // daemon
  CRON_DISPATCH_FAILED: "Cron job dispatch failed",
  WATCHDOG_PROBE_FAILED: "Health probe failed for service",

  // memory (sidecar)
  MEMORY_SIDECAR_DOWN: "Memory sidecar is not responding",
  MEMORY_SEARCH_FAILED: "Memory search returned an error",
  MEMORY_STORE_FAILED: "Failed to store memory",

  // organon (code patching)
  PATCH_FORBIDDEN_PATH: "Patch target is in a forbidden directory",
  PATCH_RATE_LIMITED: "Patch rate limit exceeded",
  PATCH_TSC_FAILED: "TypeScript compilation failed after patch",
  PATCH_TEST_FAILED: "Test suite failed after patch",
  PATCH_NOT_FOUND: "Patch ID not found in history",

  // koina (encryption)
  ENCRYPTION_NOT_INITIALIZED: "Encryption not initialized — call initEncryption first",
  ENCRYPTION_VERSION_UNSUPPORTED: "Unsupported encryption version",

  // koina (PII)
  PII_SCAN_FAILED: "PII scan threw unexpectedly",

  // portability
  PORTABILITY_IMPORT_FAILED: "Agent file version not supported for import",

  // dianoia (planning)
  PLANNING_PROJECT_NOT_FOUND: "Planning project ID does not exist",
  PLANNING_PHASE_NOT_FOUND: "Planning phase ID does not exist",
  PLANNING_REQUIREMENT_NOT_FOUND: "Planning requirement ID does not exist",
  PLANNING_STATE_CORRUPT: "Planning project state failed integrity check",
  PLANNING_CONSTRAINT_VIOLATION: "Planning operation violates a database constraint",
  PLANNING_INVALID_TRANSITION: "FSM transition is not valid from the current state",
  PLANNING_SPAWN_NOT_FOUND: "Spawn record ID does not exist",
  PLANNING_DISCUSSION_NOT_FOUND: "Discussion question ID does not exist",
  PLANNING_DUPLICATE_REQUIREMENT_ID: "Requirement ID already exists for this project",
  PLANNING_TABLE_STAKES_OUT_OF_SCOPE: "Table-stakes feature cannot be out-of-scope without rationale",
  PLANNING_PLAN_NOT_FOUND: "Plan ID not found in execution snapshot",
  PLANNING_PLAN_ID_REQUIRED: "planId is required for this operation",
  PLANNING_PHASE_ID_REQUIRED: "phaseId is required for this operation",
  PLANNING_OVERRIDE_NOTE_REQUIRED: "overrideNote is required for verification override",
  PLANNING_CHECKPOINT_ID_REQUIRED: "checkpointId is required for this operation",
  PLANNING_WORKSPACE_NOT_SET: "Workspace root not configured — call setWorkspaceRoot() first",
  PLANNING_FILE_WRITE_FAILED: "File integrity check failed after write operation",
  PLANNING_RESEARCH_ALL_FAILED: "All research dimensions failed — cannot proceed without domain research",
  PLANNING_DISPATCH_FAILED: "Plan dispatch failed after retry",
  PLANNING_DISPATCH_PARSE_FAILED: "Failed to parse dispatch response",
  PLANNING_EXECUTION_STUCK: "Plan stuck: same error pattern repeated",
  PLANNING_ITERATION_CAP_EXCEEDED: "Plan exceeded maximum iteration count",

  // Task system
  TASK_NOT_FOUND: "Task ID does not exist",
  TASK_PARENT_NOT_FOUND: "Parent task ID does not exist",
  TASK_DEP_NOT_FOUND: "Dependency task ID does not exist",
  TASK_MAX_DEPTH: "Maximum task hierarchy depth exceeded",
  TASK_CYCLE: "Dependency cycle detected",
  PLANNING_VERIFICATION_PARSE_FAILED: "Cannot extract verification check result from response",
} as const;

export type ErrorCode = keyof typeof ERROR_CODES;
