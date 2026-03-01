// Provider router — model string to provider, failover, retry with backoff
import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { createLogger } from "../koina/logger.js";
import { paths } from "../taxis/paths.js";
import { ProviderError } from "../koina/errors.js";
import {
  AnthropicProvider,
  type CompletionRequest,
  type StreamingEvent,
  type TurnResult,
} from "./anthropic.js";
import { refreshOAuthToken } from "./oauth-refresh.js";

const log = createLogger("hermeneus.router");

/** Error codes that indicate a transient server-side failure worth retrying. */
const RETRYABLE_CODES = new Set([
  "PROVIDER_INVALID_RESPONSE", // 5xx
  "PROVIDER_OVERLOADED",       // 529
  "PROVIDER_TIMEOUT",          // network/timeout
]);

/** Codes that should skip retry and go straight to credential failover. */
const FAILOVER_ONLY_CODES = new Set([
  "PROVIDER_RATE_LIMITED",     // 429 — different credential may help
  "PROVIDER_AUTH_FAILED",      // 401/403 — retry won't fix
  "PROVIDER_TOKEN_EXPIRED",    // expired OAuth — needs different credential
]);

interface RetryConfig {
  maxAttempts: number;
  baseDelayMs: number;
  maxDelayMs: number;
}

const DEFAULT_RETRY: RetryConfig = {
  maxAttempts: 3,
  baseDelayMs: 1000,
  maxDelayMs: 8000,
};

function isRetryable(error: unknown): error is ProviderError {
  return error instanceof ProviderError
    && error.recoverable
    && RETRYABLE_CODES.has(error.code)
    && !FAILOVER_ONLY_CODES.has(error.code);
}

function backoffDelay(attempt: number, config: RetryConfig): number {
  // Exponential: 1s, 2s, 4s... capped at maxDelayMs
  // Add ±20% jitter to avoid thundering herd
  const base = Math.min(config.baseDelayMs * 2 ** attempt, config.maxDelayMs);
  const jitter = base * 0.2 * (Math.random() * 2 - 1);
  return Math.round(base + jitter);
}

interface ProviderEntry {
  name: string;
  provider: AnthropicProvider;
  models: Set<string>;
}

export class ProviderRouter {
  private providers: ProviderEntry[] = [];
  private backupProviders: AnthropicProvider[] = [];
  private retryConfig: RetryConfig = DEFAULT_RETRY;

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

  /** Override retry config (useful for testing). */
  setRetryConfig(config: Partial<RetryConfig>): void {
    this.retryConfig = { ...this.retryConfig, ...config };
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

  /**
   * Retry a provider call with exponential backoff for transient 5xx/529 errors.
   * Non-retryable errors (429, 401, 403) skip retry and propagate immediately
   * so the caller can attempt credential failover instead.
   */
  private async withRetry<T>(
    label: string,
    fn: () => Promise<T>,
  ): Promise<T> {
    let lastError: unknown;
    for (let attempt = 0; attempt < this.retryConfig.maxAttempts; attempt++) {
      try {
        return await fn();
      } catch (error) {
        lastError = error;
        if (!isRetryable(error)) throw error;

        const remaining = this.retryConfig.maxAttempts - attempt - 1;
        if (remaining === 0) break;

        const delay = backoffDelay(attempt, this.retryConfig);
        log.warn(
          `${label} failed (${(error as ProviderError).code}), retrying in ${delay}ms ` +
          `(attempt ${attempt + 1}/${this.retryConfig.maxAttempts})`,
        );
        await sleep(delay);
      }
    }
    throw lastError;
  }

  /**
   * Attempt to refresh the primary OAuth token and reinitialize the provider.
   * Returns true if refresh succeeded and provider was updated.
   */
  private async attemptOAuthRefresh(entry: ProviderEntry): Promise<boolean> {
    const result = await refreshOAuthToken();
    if (!result.success || !result.newToken) {
      log.warn(`OAuth refresh failed: ${result.error}`);
      return false;
    }

    // Reinitialize the primary provider with the new token
    const refreshedProvider = new AnthropicProvider({
      authToken: result.newToken,
      label: entry.provider.label,
    });
    entry.provider = refreshedProvider;
    log.info("Primary provider reinitialized with refreshed OAuth token");
    return true;
  }

  async complete(request: CompletionRequest): Promise<TurnResult> {
    const entry = this.resolve(request.model);
    const model = request.model.includes("/") ? request.model.split("/").pop()! : request.model;
    log.debug(`Routing ${request.model} to ${entry.name} (model=${model})`);
    try {
      return await this.withRetry(
        `complete(${model})`,
        () => entry.provider.complete({ ...request, model }),
      );
    } catch (error) {
      if (!(error instanceof ProviderError) || !error.recoverable) {
        throw error;
      }

      // On token expiry, attempt refresh before falling to backup
      if (error.code === "PROVIDER_TOKEN_EXPIRED") {
        log.info("Token expired — attempting OAuth refresh before failover");
        const refreshed = await this.attemptOAuthRefresh(entry);
        if (refreshed) {
          try {
            return await entry.provider.complete({ ...request, model });
          } catch (retryErr) {
            log.warn(`Request failed after token refresh: ${retryErr instanceof Error ? retryErr.message : retryErr}`);
            // Fall through to backup credentials
          }
        }
      }

      if (this.backupProviders.length === 0) throw error;

      for (let i = 0; i < this.backupProviders.length; i++) {
        log.warn(`Primary credential exhausted retries (${error.code}), trying backup ${i + 1}/${this.backupProviders.length}`);
        try {
          return await this.withRetry(
            `complete(${model}):backup-${i + 1}`,
            () => this.backupProviders[i]!.complete({ ...request, model }),
          );
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

    // Streaming retry: collect events from the generator. If it throws a
    // retryable error before yielding message_complete, retry from scratch.
    // Events already yielded (text deltas, thinking deltas) are harmless
    // duplicates — the UI handles partial resets on reconnect.
    let lastError: unknown;
    for (let attempt = 0; attempt < this.retryConfig.maxAttempts; attempt++) {
      try {
        yield* entry.provider.completeStreaming({ ...request, model });
        return;
      } catch (error) {
        lastError = error;
        if (!isRetryable(error)) break;

        const remaining = this.retryConfig.maxAttempts - attempt - 1;
        if (remaining === 0) break;

        const delay = backoffDelay(attempt, this.retryConfig);
        log.warn(
          `stream(${model}) failed (${(error as ProviderError).code}), retrying in ${delay}ms ` +
          `(attempt ${attempt + 1}/${this.retryConfig.maxAttempts})`,
        );
        await sleep(delay);
      }
    }

    // On token expiry, attempt refresh before falling to backup
    if (lastError instanceof ProviderError && lastError.code === "PROVIDER_TOKEN_EXPIRED") {
      log.info("Token expired during streaming — attempting OAuth refresh before failover");
      const entry = this.resolve(request.model);
      const refreshed = await this.attemptOAuthRefresh(entry);
      if (refreshed) {
        try {
          yield* entry.provider.completeStreaming({ ...request, model });
          return;
        } catch (retryErr) {
          log.warn(`Streaming failed after token refresh: ${retryErr instanceof Error ? retryErr.message : retryErr}`);
          lastError = retryErr;
          // Fall through to backup credentials
        }
      }
    }

    // Primary exhausted — try backups
    if (lastError instanceof ProviderError && (lastError as ProviderError).recoverable && this.backupProviders.length > 0) {
      for (let i = 0; i < this.backupProviders.length; i++) {
        log.warn(`Primary credential exhausted retries (${(lastError as ProviderError).code}), trying backup ${i + 1}/${this.backupProviders.length}`);
        try {
          yield* this.backupProviders[i]!.completeStreaming({ ...request, model });
          return;
        } catch { /* backup also failed — try next */
          continue;
        }
      }
    }

    throw lastError;
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
    const credPath = paths.credentialFile("anthropic");
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
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") {
        log.warn(`Credential file not found: ${credPath}`);
      } else {
        log.warn(`Failed to read credential file ${credPath}: ${error instanceof Error ? error.message : error}`);
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

  // Read credential label from file (default: "primary" for file-based, "default" for env)
  let primaryLabel = "default";
  {
    const credPath2 = paths.credentialFile("anthropic");
    try {
      const raw = JSON.parse(readFileSync(credPath2, "utf-8")) as Record<string, unknown>;
      if (typeof raw["label"] === "string" && raw["label"].length > 0) {
        primaryLabel = raw["label"];
      }
    } catch { /* use default */ }
  }

  const providerOpts = authToken
    ? { authToken, label: primaryLabel }
    : fileApiKey
      ? { apiKey: fileApiKey, label: primaryLabel }
      : { label: primaryLabel };
  const anthropic = new AnthropicProvider(providerOpts);
  router.registerProvider("anthropic", anthropic, configModels);

  // Read backup credentials for failover on 429/5xx
  // Supports both legacy "backupKeys" (API key strings) and
  // "backupCredentials" (typed objects with oauth/apiKey support)
  const credPath = paths.credentialFile("anthropic");
  try {
    const raw = JSON.parse(readFileSync(credPath, "utf-8")) as Record<string, unknown>;
    const backups: AnthropicProvider[] = [];

    // New format: typed backup credentials (oauth tokens + API keys) with optional labels
    const backupCreds = raw["backupCredentials"];
    if (Array.isArray(backupCreds)) {
      for (let bi = 0; bi < backupCreds.length; bi++) {
        const cred = backupCreds[bi];
        if (typeof cred !== "object" || cred === null) continue;
        const c = cred as Record<string, unknown>;
        const label = typeof c["label"] === "string" ? c["label"] : `backup-${bi + 1}`;
        if (c["type"] === "oauth" && typeof c["token"] === "string" && (c["token"] as string).length > 0) {
          backups.push(new AnthropicProvider({ authToken: c["token"] as string, label }));
        } else if (typeof c["apiKey"] === "string" && (c["apiKey"] as string).length > 0) {
          backups.push(new AnthropicProvider({ apiKey: c["apiKey"] as string, label }));
        }
      }
    }

    // Legacy format: plain API key strings
    const backupKeys = raw["backupKeys"];
    if (Array.isArray(backupKeys)) {
      for (let bi = 0; bi < backupKeys.length; bi++) {
        const key = backupKeys[bi];
        if (typeof key === "string" && key.length > 0) {
          backups.push(new AnthropicProvider({ apiKey: key, label: `backup-${bi + 1}` }));
        }
      }
    }

    if (backups.length > 0) {
      router.registerBackupCredentials(backups);
    }
  } catch { /* no backup credentials configured */ }

  return router;
}
