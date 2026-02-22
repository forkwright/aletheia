// Canonical graph color palette — single source of truth for community/agent colors.
// Ardent dye palette leads (aima, thanatochromia, aporia), earth tones follow.

export const COMMUNITY_PALETTE = [
  "#7a2838", "#4a3860", "#5C8E63", "#9A7B4F", "#a06e3a",
  "#b07a8a", "#6b8fa3", "#7a9a8a", "#c49a6a", "#8b6a4a",
  "#6b7b6b", "#8aad6e", "#a07a5a", "#7a6b8a", "#9a8a6a",
  "#6a8a7a", "#a08060", "#8a7060", "#7a8a9a", "#6a7a5a",
] as const;

export const AGENT_COLORS: Record<string, string> = {
  syn: "#9A7B4F",       /* aged brass — primary orchestrator */
  demiurge: "#a06e3a",  /* natural leather — the maker */
  syl: "#b07a8a",       /* dusty rose */
  akron: "#5C8E63",     /* aporia green — field work */
  eiron: "#4a3860",     /* thanatochromia — analysis */
  arbor: "#6b8fa3",     /* steel blue */
  unknown: "#6b6560",   /* warm grey */
};

/** Fallback for nodes with no community assignment */
export const UNASSIGNED_COLOR = "#302c28";

export function communityColor(community: number): string {
  if (community < 0) return UNASSIGNED_COLOR;
  return COMMUNITY_PALETTE[community % COMMUNITY_PALETTE.length]!;
}

export function agentColor(agentId: string): string {
  return AGENT_COLORS[agentId] ?? AGENT_COLORS["unknown"]!;
}
