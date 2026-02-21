// Update check daemon — polls GitHub for new releases, writes to blackboard
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";

const log = createLogger("daemon:update-check");

const REPO = "forkwright/aletheia";
const CHECK_INTERVAL = 6 * 60 * 60 * 1000; // 6 hours
const INITIAL_DELAY = 60_000; // 60s after startup
const BLACKBOARD_TTL = 7 * 24 * 3600; // 1 week

export interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  latestVersion: string;
  latestTag: string;
  releaseUrl: string;
  checkedAt: string;
}

export function startUpdateChecker(
  store: SessionStore,
  currentVersion: string,
): NodeJS.Timeout {
  const check = async () => {
    try {
      const res = await fetch(
        `https://api.github.com/repos/${REPO}/releases/latest`,
        {
          headers: { "User-Agent": `aletheia/${currentVersion}` },
          signal: AbortSignal.timeout(10_000),
        },
      );

      if (!res.ok) {
        log.debug(`GitHub API returned ${res.status}`);
        return;
      }

      const release = (await res.json()) as {
        tag_name: string;
        html_url: string;
      };

      const latestVersion = release.tag_name.replace(/^v/, "");
      const available = isNewer(latestVersion, currentVersion);

      const info: UpdateInfo = {
        available,
        currentVersion,
        latestVersion,
        latestTag: release.tag_name,
        releaseUrl: release.html_url,
        checkedAt: new Date().toISOString(),
      };

      store.blackboardWrite("system:update", JSON.stringify(info), "system", BLACKBOARD_TTL);

      if (available) {
        log.info(`Update available: ${currentVersion} → ${latestVersion}`);
      }
    } catch (err) {
      log.debug(`Update check failed: ${err instanceof Error ? err.message : err}`);
    }
  };

  setTimeout(check, INITIAL_DELAY);
  return setInterval(check, CHECK_INTERVAL);
}

function isNewer(latest: string, current: string): boolean {
  const parse = (v: string) => v.split(".").map(Number);
  const [lMajor = 0, lMinor = 0, lPatch = 0] = parse(latest);
  const [cMajor = 0, cMinor = 0, cPatch = 0] = parse(current);
  if (lMajor !== cMajor) return lMajor > cMajor;
  if (lMinor !== cMinor) return lMinor > cMinor;
  return lPatch > cPatch;
}
