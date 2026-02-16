// Plugin type definitions
import type { ToolHandler } from "../organon/registry.js";
import type { AletheiaConfig } from "../taxis/schema.js";

export type HookName =
  | "onStart"
  | "onShutdown"
  | "onBeforeTurn"
  | "onAfterTurn"
  | "onBeforeDistill"
  | "onAfterDistill"
  | "onConfigReload";

export interface TurnContext {
  nousId: string;
  sessionId: string;
  messageText: string;
  media?: Array<{ contentType: string; data: string; filename?: string }>;
}

export interface TurnResult {
  nousId: string;
  sessionId: string;
  responseText: string;
  messageText: string;
  toolCalls: number;
  inputTokens: number;
  outputTokens: number;
}

export interface DistillContext {
  nousId: string;
  sessionId: string;
  messageCount: number;
  tokenCount: number;
}

export interface DistillResult {
  nousId: string;
  sessionId: string;
  factsExtracted: number;
  tokensBefore: number;
  tokensAfter: number;
}

export interface PluginApi {
  config: AletheiaConfig;
  log: (level: string, message: string) => void;
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description?: string;
}

export interface PluginDefinition {
  manifest: PluginManifest;
  tools?: ToolHandler[];
  hooks?: Partial<{
    onStart: (api: PluginApi) => Promise<void>;
    onShutdown: (api: PluginApi) => Promise<void>;
    onBeforeTurn: (api: PluginApi, context: TurnContext) => Promise<void>;
    onAfterTurn: (api: PluginApi, result: TurnResult) => Promise<void>;
    onBeforeDistill: (api: PluginApi, context: DistillContext) => Promise<void>;
    onAfterDistill: (api: PluginApi, result: DistillResult) => Promise<void>;
    onConfigReload: (api: PluginApi) => Promise<void>;
  }>;
}
