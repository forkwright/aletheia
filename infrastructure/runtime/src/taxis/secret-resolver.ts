// SecretRef resolution — replaces SecretRef objects with plain strings before any module sees the config
import { homedir } from "node:os";
import { resolve as resolvePath } from "node:path";
import { readFileSync } from "node:fs";
import { ConfigError } from "../koina/errors.js";
import { createLogger } from "../koina/logger.js";
import type { AletheiaConfig, SecretRef } from "./schema.js";

const log = createLogger("taxis:secret-resolver");

export function resolveSecretRefs(config: AletheiaConfig): AletheiaConfig {
  // Walk all provider entries — apiKey and baseUrl are credential fields
  for (const [name, provider] of Object.entries(config.models.providers)) {
    const p = provider as Record<string, unknown>;
    const base = `models.providers.${name}`;
    if (isSecretRef(p["apiKey"])) {
      p["apiKey"] = resolveRef(p["apiKey"] as SecretRef, `${base}.apiKey`);
    }
    if (isSecretRef(p["baseUrl"])) {
      p["baseUrl"] = resolveRef(p["baseUrl"] as SecretRef, `${base}.baseUrl`);
    }
  }
  // gateway.auth.token is a credential field
  const gatewayAuth = config.gateway.auth as Record<string, unknown>;
  if (isSecretRef(gatewayAuth["token"])) {
    gatewayAuth["token"] = resolveRef(
      gatewayAuth["token"] as SecretRef,
      "gateway.auth.token",
    );
  }
  log.debug("SecretRef resolution complete");
  return config;
}

function isSecretRef(value: unknown): value is SecretRef {
  return (
    value !== null &&
    typeof value === "object" &&
    "source" in (value as object) &&
    ["env", "file", "vault"].includes(
      (value as Record<string, unknown>)["source"] as string,
    )
  );
}

function resolveRef(ref: SecretRef, configPath: string): string {
  switch (ref.source) {
    case "env": {
      const val = process.env[ref.id];
      if (!val) {
        throw new ConfigError(
          `Cannot resolve SecretRef at ${configPath}: env var ${ref.id} is not set`,
          { code: "CONFIG_SECRET_UNRESOLVED", context: { configPath, source: "env", id: ref.id } },
        );
      }
      return val;
    }
    case "file": {
      const expanded = expandTilde(ref.id);
      let contents: string;
      try {
        contents = readFileSync(expanded, "utf-8");
      } catch (err) {
        throw new ConfigError(
          `Cannot resolve SecretRef at ${configPath}: file not readable: ${expanded}`,
          { code: "CONFIG_SECRET_UNRESOLVED", context: { configPath, source: "file", id: ref.id }, cause: err },
        );
      }
      const trimmed = contents.replace(/\n$/, "");
      if (trimmed.length === 0) {
        throw new ConfigError(
          `Cannot resolve SecretRef at ${configPath}: file is empty: ${expanded}`,
          { code: "CONFIG_SECRET_UNRESOLVED", context: { configPath, source: "file", id: ref.id } },
        );
      }
      return trimmed;
    }
    case "vault": {
      throw new ConfigError(
        `Cannot resolve SecretRef at ${configPath}: Vault source is not yet supported. A plugin interface is planned for future versions.`,
        { code: "CONFIG_SECRET_VAULT_UNSUPPORTED", context: { configPath, source: "vault" } },
      );
    }
  }
}

function expandTilde(filePath: string): string {
  if (filePath === "~") return homedir();
  if (filePath.startsWith("~/")) return resolvePath(homedir(), filePath.slice(2));
  return resolvePath(filePath);
}
