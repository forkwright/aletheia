// Provider router — model string to provider, failover
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { ProviderError } from "../koina/errors.js";
import {
  AnthropicProvider,
  type CompletionRequest,
  type StreamingEvent,
  type TurnResult,
} from "./anthropic.js";

const log = createLogger("hermeneus.router");

interface ProviderEntry {
  name: string;
  provider: AnthropicProvider;
  models: Set<string>;
}

export class ProviderRouter {
  private providers: ProviderEntry[] = [];
  private backupProviders: AnthropicProvider[] = [];

  registerProvider(
    name: string,
    provider: AnthropicProvider,
    models: string[],
  ): void {
    this.providers.push({
      name,
      provider,
      models: new Set(models),
    });
    log.info(`Registered provider ${name}${models.length > 0 ? ` (${models.join(", ")})` : ""}`);
  }

  registerBackupCredentials(providers: AnthropicProvider[]): void {
    this.backupProviders = providers;
    if (providers.length > 0) {
      log.info(`Registered ${providers.length} backup credential(s) for failover`);
    }
  }

  private resolve(model: string): ProviderEntry {
    for (const entry of this.providers) {
      if (entry.models.has(model)) return entry;
    }
    // Strip provider prefix (e.g. "anthropic/claude-opus-4-6" → "claude-opus-4-6")
    const stripped = model.includes("/") ? model.split("/").pop()! : model;
    if (stripped !== model) {
      for (const entry of this.providers) {
        if (entry.models.has(stripped)) return entry;
      }
    }
    // Fallback: any claude-* model goes to first provider (Anthropic)
    if ((model.startsWith("claude-") || stripped.startsWith("claude-")) && this.providers.length > 0) {
      return this.providers[0]!;
    }
    throw new ProviderError(`No provider found for model: ${model}`, {
      code: "PROVIDER_NOT_FOUND", context: { model },
    });
  }

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const entry = this.resolve(request.model);
    const model = request.model.includes("/") ? request.model.split("/").pop()! : request.model;
    log.debug(`Routing ${request.model} to ${entry.name} (model=${model})`);
    try {
      return await entry.provider.complete({ ...request, model });
    } catch (error) {
      if (!(error instanceof ProviderError) || !error.recoverable || this.backupProviders.length === 0) {
        throw error;
      }
      for (let i = 0; i < this.backupProviders.length; i++) {
        log.warn(`Primary credential failed (${error.code}), trying backup ${i + 1}/${this.backupProviders.length}`);
        try {
          return await this.backupProviders[i]!.complete({ ...request, model });
        } catch { /* backup also failed — try next */
          continue;
        }
      }
      throw error;
    }
  }

  async *completeStreaming(request: CompletionRequest): AsyncGenerator<StreamingEvent> {
    const entry = this.resolve(request.model);
    const model = request.model.includes("/") ? request.model.split("/").pop()! : request.model;
    log.debug(`Streaming ${request.model} via ${entry.name} (model=${model})`);
    yield* entry.provider.completeStreaming({ ...request, model });
  }

  async completeWithFailover(
    request: CompletionRequest,
    fallbackModels: string[],
  ): Promise<TurnResult> {
    try {
      return await this.complete(request);
    } catch (error) {
      for (const fallback of fallbackModels) {
        log.warn(
          `Primary model ${request.model} failed, trying ${fallback}`,
        );
        try {
          return await this.complete({ ...request, model: fallback });
        } catch { /* fallback model also failed — try next */
          continue;
        }
      }
      throw error;
    }
  }

}

export interface RouterConfig {
  providers?: Record<string, { models?: Array<{ id: string }> }>;
}

export function createDefaultRouter(config?: RouterConfig): ProviderRouter {
  const router = new ProviderRouter();

  // Resolve Anthropic credentials: env vars take priority, then credential file.
  // Credential file supports both "apiKey" (API key) and "token" (OAuth) fields.
  let authToken: string | undefined;
  let fileApiKey: string | undefined;
  const envAuthToken = process.env["ANTHROPIC_AUTH_TOKEN"];
  const envApiKey = process.env["ANTHROPIC_API_KEY"];

  if (!envAuthToken && !envApiKey) {
    const home = process.env["HOME"] ?? "/tmp";
    const credPath = join(home, ".aletheia", "credentials", "anthropic.json");
    try {
      const raw = readFileSync(credPath, "utf-8");
      let creds: Record<string, unknown> | undefined;
      try {
        creds = JSON.parse(raw) as Record<string, unknown>;
      } catch {
        log.warn(`Credential file ${credPath} contains invalid JSON — skipping`);
      }
      if (creds && typeof creds === "object") {
        const token = creds["token"];
        const apiKey = creds["apiKey"];
        if (typeof token === "string" && token.length > 0) {
          authToken = token;
          log.info("Loaded OAuth token from credentials");
        } else if (typeof apiKey === "string" && apiKey.length > 0) {
          fileApiKey = apiKey;
          log.info("Loaded API key from credentials");
        } else {
          log.warn(`Credential file ${credPath} has no valid "apiKey" or "token" field`);
        }
      }
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        log.warn(`Credential file not found: ${credPath}`);
      } else {
        log.warn(`Failed to read credential file ${credPath}: ${err instanceof Error ? err.message : err}`);
      }
    }
  } else {
    log.info(`Using ${envAuthToken ? "ANTHROPIC_AUTH_TOKEN" : "ANTHROPIC_API_KEY"} from environment`);
  }

  if (!authToken && !fileApiKey && !envAuthToken && !envApiKey) {
    log.error("No Anthropic authentication configured — no env vars, no credential file. API calls WILL fail.");
  }

  // Use model IDs from config if available, otherwise empty list
  // (the router's claude-* fallback handles unregistered models)
  const configModels = config?.providers?.["anthropic"]?.models?.map((m) => m.id) ?? [];

  const providerOpts = authToken ? { authToken } : fileApiKey ? { apiKey: fileApiKey } : undefined;
  const anthropic = new AnthropicProvider(providerOpts);
  router.registerProvider("anthropic", anthropic, configModels);

  // Read backup credentials for failover on 429/5xx
  const home = process.env["HOME"] ?? "/tmp";
  const credPath = join(home, ".aletheia", "credentials", "anthropic.json");
  try {
    const raw = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>;
    const backupKeys = raw["backupKeys"];
    if (Array.isArray(backupKeys)) {
      const backups = backupKeys
        .filter((k): k is string => typeof k === "string" && k.length > 0)
        .map((key) => new AnthropicProvider({ apiKey: key }));
      if (backups.length > 0) {
        router.registerBackupCredentials(backups);
      }
    }
  } catch { /* no backup keys configured */ }

  return router;
}
