// Provider router — model string to provider, failover
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
    log.info(`Registered provider ${name} with ${models.length} models`);
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

export function createDefaultRouter(): ProviderRouter {
  const router = new ProviderRouter();
  const anthropic = new AnthropicProvider();
  router.registerProvider("anthropic", anthropic, [
    "claude-opus-4-6",
    "claude-sonnet-4-5-20250929",
    "claude-haiku-4-5-20251001",
  ]);
  return router;
}
