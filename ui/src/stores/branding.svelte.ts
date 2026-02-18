import { fetchBranding, type Branding } from "../lib/api";

const DEFAULT: Branding = { name: "Aletheia" };

let branding = $state<Branding>({ ...DEFAULT });
let loaded = $state(false);

export function getBranding(): Branding {
  return branding;
}

export function getBrandName(): string {
  return branding.name;
}

export async function loadBranding(): Promise<void> {
  if (loaded) return;
  try {
    branding = await fetchBranding();
    document.title = branding.name;
    if (branding.favicon) {
      const link = document.querySelector("link[rel='icon']") as HTMLLinkElement | null;
      if (link) link.href = branding.favicon;
    }
  } catch {
    // API unavailable — keep defaults
  }
  loaded = true;
}
