// Credential config store — tracks active credential label and config for topbar pill
import { getEffectiveToken } from "../lib/api";

interface CredentialEntry {
  label: string;
  type: "oauth" | "api" | "unknown";
}

interface CredentialConfig {
  primary: CredentialEntry;
  backups: CredentialEntry[];
}

let credentialConfig = $state<CredentialConfig | null>(null);
let activeCredentialLabel = $state<string>("");

export function getCredentialConfig(): CredentialConfig | null {
  return credentialConfig;
}

export function getActiveCredentialLabel(): string {
  return activeCredentialLabel || credentialConfig?.primary.label || "";
}

export function setActiveCredentialLabel(label: string): void {
  activeCredentialLabel = label;
}

export async function loadCredentialConfig(): Promise<void> {
  try {
    const token = getEffectiveToken();
    const res = await fetch("/api/system/credentials", {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (res.ok) {
      credentialConfig = await res.json() as CredentialConfig;
      if (!activeCredentialLabel) {
        activeCredentialLabel = credentialConfig.primary.label;
      }
    }
  } catch {
    // Non-fatal — pill simply won't render
  }
}
