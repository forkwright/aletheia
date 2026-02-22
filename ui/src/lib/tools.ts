// Canonical tool-to-category mapping — single source of truth for UI display
export interface ToolCategory {
  icon: string;
  label: string;
}

export const TOOL_CATEGORIES: Record<string, ToolCategory> = {
  // Filesystem
  read: { icon: "\u{1F4C1}", label: "fs" },
  write: { icon: "\u{1F4C1}", label: "fs" },
  edit: { icon: "\u{1F4C1}", label: "fs" },
  ls: { icon: "\u{1F4C1}", label: "fs" },

  // Search
  find: { icon: "\u{1F50D}", label: "search" },
  grep: { icon: "\u{1F50D}", label: "search" },
  web_search: { icon: "\u{1F50D}", label: "search" },
  mem0_search: { icon: "\u{1F50D}", label: "search" },

  // Execution
  exec: { icon: "\u26A1", label: "exec" },

  // Communication
  sessions_send: { icon: "\u{1F4AC}", label: "comms" },
  sessions_ask: { icon: "\u{1F4AC}", label: "comms" },
  sessions_spawn: { icon: "\u{1F4AC}", label: "comms" },
  message: { icon: "\u{1F4AC}", label: "comms" },

  // System
  blackboard: { icon: "\u{1F9E0}", label: "system" },
  note: { icon: "\u{1F9E0}", label: "system" },
  enable_tool: { icon: "\u{1F9E0}", label: "system" },

  // Web
  web_fetch: { icon: "\u{1F310}", label: "web" },
};

const DEFAULT_CATEGORY: ToolCategory = { icon: "\u2699", label: "other" };

export function getToolCategory(name: string): ToolCategory {
  return TOOL_CATEGORIES[name] ?? DEFAULT_CATEGORY;
}
