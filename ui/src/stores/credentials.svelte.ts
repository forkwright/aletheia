// Credential label store — tracks which credential was used on the last turn
import { fetchCredentialInfo, type CredentialInfo } from "../lib/api";

let activeLabel = $state<string>("primary");
let credentialConfig = $state<CredentialInfo | null>(null);
let loaded = $state(false);

export function getActiveCredentialLabel(): string {
  return activeLabel;
}

export function setActiveCredentialLabel(label: string): void {
  activeLabel = label;
}

export function getCredentialConfig(): CredentialInfo | null {
  return credentialConfig;
}

export function isCredentialConfigLoaded(): boolean {
  return loaded;
}

export async function loadCredentialConfig(): Promise<void> {
  try {
    credentialConfig = await fetchCredentialInfo();
    // Set initial label to primary
    activeLabel = credentialConfig.primary.label;
    loaded = true;
  } catch {
    // Fallback — no credential endpoint available
    loaded = true;
  }
}
