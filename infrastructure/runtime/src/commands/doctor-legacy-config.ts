import type { AletheiaConfig } from "../config/config.js";
export function normalizeLegacyConfigValues(cfg: AletheiaConfig): {
  config: AletheiaConfig;
  changes: string[];
} {
  return { config: cfg, changes: [] };
}
