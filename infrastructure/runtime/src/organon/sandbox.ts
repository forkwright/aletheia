// Command pre-screen — deny dangerous patterns before execution
import { createLogger } from "../koina/logger.js";

const log = createLogger("sandbox");

const DEFAULT_DENY_PATTERNS = [
  "rm -rf /",
  "rm -rf /*",
  "sudo *",
  "chmod +s *",
  "chmod u+s *",
  "mkfs*",
  "dd if=*of=/dev/*",
  "> /dev/sd*",
  "shutdown*",
  "reboot*",
  "systemctl stop *",
  "systemctl disable *",
  ":(){ :|:&};:",
  "curl *| bash",
  "curl *|bash",
  "wget *| bash",
  "wget *|bash",
];

export interface ScreenResult {
  allowed: boolean;
  matchedPattern?: string;
}

export function screenCommand(
  command: string,
  extraDenyPatterns: string[] = [],
  removeDenyPatterns: string[] = [],
): ScreenResult {
  const removeSet = new Set(removeDenyPatterns);
  const defaults = DEFAULT_DENY_PATTERNS.filter((p) => !removeSet.has(p));
  const patterns = [...defaults, ...extraDenyPatterns];
  const normalized = command.replace(/\s+/g, " ").trim();

  for (const pattern of patterns) {
    if (matchGlob(normalized, pattern)) {
      log.warn(`Blocked command matching "${pattern}": ${normalized.slice(0, 100)}`);
      return { allowed: false, matchedPattern: pattern };
    }
  }

  return { allowed: true };
}

function matchGlob(text: string, pattern: string): boolean {
  const escaped = pattern
    .replace(/[.+^${}()|[\]\\]/g, "\\$&")
    .replace(/\*/g, ".*");
  const re = new RegExp(`^${escaped}$`, "i");
  return re.test(text);
}

export function getDefaultDenyPatterns(): string[] {
  return [...DEFAULT_DENY_PATTERNS];
}
