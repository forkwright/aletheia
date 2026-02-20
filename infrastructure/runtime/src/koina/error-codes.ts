// Canonical error code registry — every error code in the system is defined here
// Format: MODULE_CONDITION
// AI agents: search this file to understand what errors a module can produce.

export const ERROR_CODES = {
  // hermeneus (provider routing)
  PROVIDER_TIMEOUT: "API call timed out",
  PROVIDER_RATE_LIMITED: "Rate limited by provider",
  PROVIDER_AUTH_FAILED: "Authentication failed",
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
  PIPELINE_STREAM_INCOMPLETE: "Streaming pipeline ended without message_complete",
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

  // semeion (Signal)
  SIGNAL_DAEMON_DOWN: "Signal CLI daemon is not running",
  SIGNAL_SEND_FAILED: "Failed to send Signal message",
  SIGNAL_PARSE_ERROR: "Could not parse inbound Signal message",
  SIGNAL_SSE_FAILED: "SSE connection to Signal daemon failed",
  SIGNAL_RPC_ERROR: "Signal RPC call returned an error",

  // taxis (config)
  CONFIG_NOT_FOUND: "Configuration file not found at expected path",
  CONFIG_VALIDATION_FAILED: "Configuration failed schema validation",
  CONFIG_MISSING_REQUIRED: "Required configuration field is missing",

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
} as const;

export type ErrorCode = keyof typeof ERROR_CODES;
