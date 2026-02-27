// Load and write the bootstrap anchor.json that pins nous.dir and deploy.dir
import { homedir } from "node:os";
import { join } from "node:path";
import { z } from "zod";
import { readJson, writeJson } from "../koina/fs.js";
import { createLogger } from "../koina/logger.js";
import { ConfigError } from "../koina/errors.js";

const log = createLogger("taxis:bootstrap");

const AnchorSchema = z
  .object({ nousDir: z.string(), deployDir: z.string() })
  .passthrough();

export type BootstrapAnchor = z.infer<typeof AnchorSchema>;

export function anchorPath(): string {
  return join(homedir(), ".aletheia", "anchor.json");
}

export function loadBootstrapAnchor(): { anchor: BootstrapAnchor; path: string } {
  const path = anchorPath();
  const raw = readJson(path);

  if (raw === null) {
    log.debug("anchor.json absent — exiting with init prompt");
    throw new ConfigError(
      "anchor.json not found — run 'aletheia init' to configure your deployment",
      { code: "CONFIG_ANCHOR_NOT_FOUND", context: { path } },
    );
  }

  const result = AnchorSchema.safeParse(raw);
  if (!result.success) {
    throw new ConfigError(
      "anchor.json invalid: " +
        result.error.issues
          .map((i) => i.path.join(".") + ": " + i.message)
          .join(", "),
      { code: "CONFIG_ANCHOR_INVALID", context: { path } },
    );
  }

  const KNOWN_ANCHOR_KEYS = new Set(["nousDir", "deployDir", "$comment"]);
  for (const key of Object.keys(raw as Record<string, unknown>)) {
    if (!KNOWN_ANCHOR_KEYS.has(key)) {
      log.warn(`Unknown key "${key}" in anchor.json — ignored (forward-compatible)`);
    }
  }

  return { anchor: result.data, path };
}

export function writeBootstrapAnchor(nousDir: string, deployDir: string): void {
  const path = anchorPath();
  writeJson(path, {
    $comment:
      "Fixed anchor for Aletheia path resolution. Edit directly or run 'aletheia init'. Restart daemon after changes.",
    nousDir,
    deployDir,
  });
}
