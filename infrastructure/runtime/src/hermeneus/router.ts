// Provider router — model string to provider, failover
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { ProviderError } from "../koina/errors.js";
import {
  AnthropicProvider,
  type CompletionRequest,
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
    throw new ProviderError(`No provider found for model: ${model}`);
  }

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const entry = this.resolve(request.model);
    // Normalize model name — strip provider prefix for the SDK
    const model = request.model.includes("/") ? request.model.split("/").pop()! : request.model;
    log.debug(`Routing ${request.model} to ${entry.name} (model=${model})`);
    return entry.provider.complete({ ...request, model });
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
        } catch {
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

  // Load OAuth token from credentials if ANTHROPIC_AUTH_TOKEN not in env
  let authToken: string | undefined;
  const envAuthToken = process.env.ANTHROPIC_AUTH_TOKEN;
  const envApiKey = process.env.ANTHROPIC_API_KEY;

  if (!envAuthToken && !envApiKey) {
    const home = process.env.HOME ?? "/home/syn";
    const credPath = join(home, ".aletheia", "credentials", "anthropic.json");
    try {
      const raw = readFileSync(credPath, "utf-8");
      let creds: unknown;
      try {
        creds = JSON.parse(raw);
      } catch {
        log.warn(`Credential file ${credPath} contains invalid JSON — skipping`);
      }
      if (creds && typeof creds === "object" && "token" in (creds as Record<string, unknown>)) {
        const token = (creds as Record<string, string>).token;
        if (typeof token === "string" && token.length > 0) {
          authToken = token;
          log.info("Loaded OAuth token from credentials");
        } else {
          log.warn(`Credential file ${credPath} has empty or non-string token field`);
        }
      } else {
        log.warn(`Credential file ${credPath} missing "token" field — expected { "token": "sk-ant-..." }`);
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

  if (!authToken && !envAuthToken && !envApiKey) {
    log.error("No Anthropic authentication configured — no env vars, no credential file. API calls WILL fail.");
  }

  // Use model IDs from config if available, otherwise empty list
  // (the router's claude-* fallback handles unregistered models)
  const configModels = config?.providers?.["anthropic"]?.models?.map((m) => m.id) ?? [];

  const anthropic = new AnthropicProvider(authToken ? { authToken } : undefined);
  router.registerProvider("anthropic", anthropic, configModels);
  return router;
}
