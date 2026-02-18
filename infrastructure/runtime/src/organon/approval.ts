// Tool approval gate — pauses execution for user confirmation on risky operations

import { getReversibility, type Reversibility } from "./reversibility.js";

export type ApprovalMode = "autonomous" | "guarded" | "supervised";

export interface ApprovalDecision {
  decision: "approve" | "deny";
  alwaysAllow?: boolean;
}

// Dangerous patterns for exec commands — only truly destructive operations
const DANGEROUS_EXEC_PATTERNS = [
  /\brm\s+(-[a-zA-Z]*r[a-zA-Z]*|--recursive)\b/,  // rm -r, rm -rf, rm -Rf
  /\brm\s+-[a-zA-Z]*f[a-zA-Z]*\b.*\//,              // rm -f with path
  /\bdd\b\s+.*of=/,                                   // dd with output file
  /\bmkfs\b/,                                          // format filesystem
  /\bshutdown\b/,                                      // system shutdown
  /\breboot\b/,                                        // system reboot
  /\bgit\s+push\s+.*--force\b/,                       // force push
  /\bgit\s+push\s+-f\b/,                              // force push short
  /\bgit\s+reset\s+--hard\b/,                         // hard reset
  /\bgit\s+clean\s+-[a-zA-Z]*f/,                      // git clean -f
  /\bDROP\s+(TABLE|DATABASE|SCHEMA)\b/i,              // SQL drops
  /\bTRUNCATE\s+TABLE\b/i,                            // SQL truncate
  /\bDELETE\s+FROM\b(?!.*\bWHERE\b)/i,               // DELETE without WHERE
  /\bcurl\b.*\b(POST|PUT|DELETE|PATCH)\b/i,           // HTTP mutations via curl
  /\bchmod\s+777\b/,                                   // world-writable permissions
  /\bchown\s+-R\b/,                                    // recursive ownership change
];

const SAFE_READ_TOOLS = new Set([
  "file_read", "read", "grep", "find", "ls",
  "web_search", "brave_search", "web_fetch",
  "mem0_search", "plan_status", "config_read",
  "session_status", "context_check", "trace_lookup",
  "tool_list_authored", "check_calibration",
  "what_do_i_know", "recent_corrections", "status_report",
  "enable_tool", "blackboard", "browser",
]);

export interface ApprovalCheck {
  required: boolean;
  reason?: string;
  risk: Reversibility;
}

export function requiresApproval(
  toolName: string,
  input: Record<string, unknown>,
  mode: ApprovalMode,
  sessionAllowList?: Set<string>,
): ApprovalCheck {
  const risk = getReversibility(toolName);

  if (mode === "autonomous") {
    return { required: false, risk };
  }

  // Session-level allow list (user clicked "always allow" for this tool)
  if (sessionAllowList?.has(toolName)) {
    return { required: false, risk };
  }

  if (mode === "supervised") {
    if (SAFE_READ_TOOLS.has(toolName)) {
      return { required: false, risk };
    }
    return { required: true, reason: `Supervised mode: all non-read tools require approval`, risk };
  }

  // Guarded mode — only truly dangerous operations
  if (risk === "destructive") {
    return { required: true, reason: `Destructive operation: ${toolName}`, risk };
  }

  if (toolName === "exec") {
    const cmd = String(input["command"] ?? "");
    for (const pattern of DANGEROUS_EXEC_PATTERNS) {
      if (pattern.test(cmd)) {
        return { required: true, reason: `Dangerous command pattern detected`, risk: "destructive" };
      }
    }
    return { required: false, risk };
  }

  // Messages to external recipients always pause in guarded mode
  if (toolName === "message" || toolName === "voice_reply") {
    return { required: true, reason: `Sending external message`, risk };
  }

  // Sessions send to other agents — irreversible but within system
  if (toolName === "sessions_send") {
    return { required: false, risk };
  }

  return { required: false, risk };
}

// Approval channel — pending approvals waiting for user response
interface PendingApproval {
  resolve: (decision: ApprovalDecision) => void;
  toolName: string;
  toolId: string;
  input: unknown;
  risk: Reversibility;
  createdAt: number;
}

export class ApprovalGate {
  private pending = new Map<string, PendingApproval>();
  // Per-session tool allow lists (populated by "always allow" decisions)
  private sessionAllowLists = new Map<string, Set<string>>();

  private key(turnId: string, toolId: string): string {
    return `${turnId}:${toolId}`;
  }

  getSessionAllowList(sessionId: string): Set<string> | undefined {
    return this.sessionAllowLists.get(sessionId);
  }

  addToSessionAllowList(sessionId: string, toolName: string): void {
    if (!this.sessionAllowLists.has(sessionId)) {
      this.sessionAllowLists.set(sessionId, new Set());
    }
    this.sessionAllowLists.get(sessionId)!.add(toolName);
  }

  async waitForApproval(
    turnId: string,
    toolId: string,
    toolName: string,
    input: unknown,
    risk: Reversibility,
    signal?: AbortSignal,
  ): Promise<ApprovalDecision> {
    const k = this.key(turnId, toolId);

    return new Promise<ApprovalDecision>((resolve, reject) => {
      this.pending.set(k, {
        resolve,
        toolName,
        toolId,
        input,
        risk,
        createdAt: Date.now(),
      });

      // If abort signal fires while waiting, clean up and reject
      const onAbort = () => {
        this.pending.delete(k);
        reject(new Error("Approval cancelled — turn aborted"));
      };
      signal?.addEventListener("abort", onAbort, { once: true });
    });
  }

  resolveApproval(turnId: string, toolId: string, decision: ApprovalDecision): boolean {
    const k = this.key(turnId, toolId);
    const entry = this.pending.get(k);
    if (!entry) return false;

    this.pending.delete(k);
    entry.resolve(decision);
    return true;
  }

  hasPending(turnId: string, toolId: string): boolean {
    return this.pending.has(this.key(turnId, toolId));
  }

  getPendingCount(): number {
    return this.pending.size;
  }

  // Clean up stale approvals (e.g. if a turn crashed without resolving)
  expireStale(maxAgeMs: number = 5 * 60 * 1000): number {
    const now = Date.now();
    let expired = 0;
    for (const [k, entry] of this.pending) {
      if (now - entry.createdAt > maxAgeMs) {
        entry.resolve({ decision: "deny" });
        this.pending.delete(k);
        expired++;
      }
    }
    return expired;
  }
}
