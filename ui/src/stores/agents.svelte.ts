import { fetchAgents, fetchAgentIdentity } from "../lib/api";
import type { Agent } from "../lib/types";

let agents = $state<Agent[]>([]);
let activeAgentId = $state<string | null>(null);
let loading = $state(false);
const identityCache = new Map<string, { name: string; emoji: string | null }>();

export function getAgents(): Agent[] {
  return agents;
}

export function getActiveAgent(): Agent | null {
  return agents.find((a) => a.id === activeAgentId) ?? null;
}

export function getActiveAgentId(): string | null {
  return activeAgentId;
}

export function isLoading(): boolean {
  return loading;
}

export async function loadAgents(): Promise<void> {
  loading = true;
  try {
    const list = await fetchAgents();
    // Enrich with identity (emoji)
    for (const agent of list) {
      if (!identityCache.has(agent.id)) {
        try {
          const identity = await fetchAgentIdentity(agent.id);
          identityCache.set(agent.id, identity);
        } catch {
          identityCache.set(agent.id, { name: agent.name, emoji: null });
        }
      }
      const cached = identityCache.get(agent.id);
      if (cached) {
        agent.name = cached.name || agent.name;
        agent.emoji = cached.emoji;
      }
    }
    agents = list;
    if (!activeAgentId && list.length > 0) {
      activeAgentId = list[0]!.id;
    }
  } finally {
    loading = false;
  }
}

export function setActiveAgent(id: string): void {
  activeAgentId = id;
}

export function getAgentEmoji(id: string): string | null {
  return identityCache.get(id)?.emoji ?? null;
}
