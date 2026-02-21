// Tool reversibility tagging — enables counterfactual simulation for irreversible actions

export type Reversibility = "reversible" | "irreversible" | "destructive";

// Default reversibility by tool name — tools not listed default to "reversible"
const TOOL_REVERSIBILITY: Record<string, Reversibility> = {
  // Filesystem — writes are irreversible (no undo), reads are safe
  exec: "irreversible",
  file_write: "irreversible",
  file_edit: "irreversible",
  file_read: "reversible",
  grep: "reversible",
  find: "reversible",
  ls: "reversible",

  // Web — read-only is safe, fetch can have side effects
  web_fetch: "reversible",
  web_search: "reversible",
  brave_search: "reversible",

  // Communication — messages cannot be unsent
  message: "irreversible",
  voice_reply: "irreversible",
  sessions_send: "irreversible",
  sessions_ask: "reversible", // ask is just a query

  // Memory — mutations are irreversible
  mem0_search: "reversible",
  fact_retract: "destructive",
  memory_correct: "destructive",
  memory_forget: "destructive",

  // Planning — state changes but recoverable
  plan_create: "reversible",
  plan_propose: "reversible",
  plan_status: "reversible",
  plan_step_complete: "irreversible",
  plan_step_fail: "irreversible",

  // Browser — navigating is ephemeral
  browser: "reversible",

  // Config — read only
  config_read: "reversible",
  session_status: "reversible",

  // Self-authoring — creates persistent files
  tool_create: "irreversible",
  tool_record_failure: "irreversible",
  tool_list_authored: "reversible",
};

export function getReversibility(toolName: string): Reversibility {
  return TOOL_REVERSIBILITY[toolName] ?? "reversible";
}

export function requiresSimulation(toolName: string, input: Record<string, unknown>): boolean {
  const rev = getReversibility(toolName);
  if (rev === "destructive") return true;

  if (rev === "irreversible") {
    // Message to non-operator always simulates
    if (toolName === "message" || toolName === "voice_reply") {
      return true;
    }
    // Exec with destructive patterns
    if (toolName === "exec") {
      const cmd = String(input["command"] ?? "");
      if (/\brm\s+-rf\b|\bdd\b|\bmkfs\b|\bshutdown\b|\breboot\b/.test(cmd)) {
        return true;
      }
    }
  }

  return false;
}

export function buildSimulationPrompt(toolName: string, input: Record<string, unknown>): string {
  const rev = getReversibility(toolName);
  return [
    `You are about to execute tool "${toolName}" (reversibility: ${rev}).`,
    `Input: ${JSON.stringify(input, null, 2).slice(0, 500)}`,
    "",
    "Before executing, briefly assess:",
    "1. What will this action do?",
    "2. Can it be undone? What are the risks?",
    "3. Should you proceed? (yes/no with reasoning)",
    "",
    "Respond with a JSON object: { \"proceed\": true/false, \"reasoning\": \"...\" }",
  ].join("\n");
}
