// Zod schemas — single source of truth for config types
import { z } from "zod";

const ModelSpec = z.union([
  z.string(),
  z.object({
    primary: z.string(),
    fallbacks: z.array(z.string()).default([]),
  }),
]);

const HeartbeatConfig = z
  .object({
    every: z.string().default("45m"),
    activeHours: z
      .object({
        start: z.string().default("08:00"),
        end: z.string().default("23:00"),
        timezone: z.string().default("America/Chicago"),
      })
      .default({}),
    model: z.string().optional(),
    session: z.string().default("main"),
    prompt: z.string().optional(),
  })
  .default({});

const CompactionConfig = z
  .object({
    mode: z.enum(["default", "safeguard"]).default("default"),
    reserveTokensFloor: z.number().default(8000),
    maxHistoryShare: z.number().default(0.7),
    memoryFlush: z
      .object({
        enabled: z.boolean().default(true),
        softThresholdTokens: z.number().default(8000),
        prompt: z.string().optional(),
        systemPrompt: z.string().optional(),
      })
      .default({}),
  })
  .default({});

const ToolsConfig = z
  .object({
    profile: z
      .enum(["minimal", "coding", "messaging", "full"])
      .default("full"),
    allow: z.array(z.string()).default([]),
    deny: z.array(z.string()).default([]),
  })
  .default({});

const SubagentConfig = z
  .object({
    allowAgents: z.array(z.string()).default([]),
    model: ModelSpec.optional(),
  })
  .default({});

const NousDefinition = z.object({
  id: z.string(),
  default: z.boolean().default(false),
  name: z.string().optional(),
  workspace: z.string(),
  model: ModelSpec.optional(),
  subagents: SubagentConfig.default({}),
  tools: ToolsConfig.default({}),
  heartbeat: HeartbeatConfig.optional(),
  identity: z
    .object({
      name: z.string().optional(),
      emoji: z.string().optional(),
    })
    .optional(),
}).passthrough();

const AgentsConfig = z.object({
  defaults: z
    .object({
      model: z
        .object({
          primary: z.string().default("claude-opus-4-6"),
          fallbacks: z.array(z.string()).default([]),
        })
        .default({}),
      workspace: z.string().optional(),
      bootstrapMaxChars: z.number().default(40000),
      userTimezone: z.string().default("America/Chicago"),
      contextTokens: z.number().default(200000),
      compaction: CompactionConfig.default({}),
      heartbeat: HeartbeatConfig.optional(),
      tools: ToolsConfig.default({}),
      timeoutSeconds: z.number().default(300),
    })
    .passthrough()
    .default({}),
  list: z.array(NousDefinition).default([]),
});

const BindingMatch = z.object({
  channel: z.string(),
  accountId: z.string().optional(),
  peer: z
    .object({
      kind: z.string(),
      id: z.string(),
    })
    .optional(),
});

const Binding = z.object({
  agentId: z.string(),
  match: BindingMatch,
});

const SignalAccountConfig = z.object({
  name: z.string().optional(),
  enabled: z.boolean().default(true),
  account: z.string().optional(),
  httpUrl: z.string().optional(),
  httpHost: z.string().default("localhost"),
  httpPort: z.number().default(8080),
  cliPath: z.string().optional(),
  autoStart: z.boolean().default(true),
  receiveMode: z.enum(["on-start", "manual"]).default("on-start"),
  sendReadReceipts: z.boolean().default(true),
  dmPolicy: z
    .enum(["pairing", "allowlist", "open", "disabled"])
    .default("open"),
  groupPolicy: z
    .enum(["open", "disabled", "allowlist"])
    .default("allowlist"),
  allowFrom: z.array(z.union([z.string(), z.number()])).default([]),
  groupAllowFrom: z.array(z.union([z.string(), z.number()])).default([]),
  textChunkLimit: z.number().default(2000),
  mediaMaxMb: z.number().default(25),
  requireMention: z.boolean().default(true),
});

// Signal config supports both flat format (v1 compat) and accounts map (v2).
// Flat: { enabled, account, cliPath, dmPolicy, ... }
// Nested: { enabled, accounts: { "default": { account, cliPath, dmPolicy, ... } } }
const SignalConfig = z.preprocess(
  (val) => {
    if (val && typeof val === "object" && !Array.isArray(val)) {
      const obj = val as Record<string, unknown>;
      // If 'account' exists at top level but 'accounts' doesn't, lift flat format
      if ("account" in obj && !("accounts" in obj)) {
        const { enabled, ...rest } = obj;
        return { enabled: enabled ?? true, accounts: { default: rest } };
      }
    }
    return val;
  },
  z.object({
    enabled: z.boolean().default(true),
    accounts: z.record(z.string(), SignalAccountConfig).default({}),
  }),
).default({ enabled: true, accounts: {} });

const ChannelsConfig = z
  .object({
    signal: SignalConfig,
  })
  .default({});

const GatewayConfig = z
  .object({
    port: z.number().default(18789),
    bind: z
      .enum(["auto", "lan", "loopback", "custom"])
      .default("lan"),
    auth: z
      .object({
        mode: z.enum(["token", "password"]).default("token"),
        token: z.string().optional(),
      })
      .default({}),
    controlUi: z
      .object({
        enabled: z.boolean().default(true),
        allowInsecureAuth: z.boolean().default(false),
      })
      .default({}),
  })
  .passthrough()
  .default({});

const PluginsConfig = z
  .object({
    enabled: z.boolean().default(true),
    load: z
      .object({
        paths: z.array(z.string()).default([]),
      })
      .default({}),
    entries: z
      .record(
        z.string(),
        z.object({
          enabled: z.boolean().default(true),
          config: z.record(z.string(), z.unknown()).default({}),
        }),
      )
      .default({}),
  })
  .default({});

const SessionConfig = z
  .object({
    scope: z.enum(["per-sender", "global"]).default("per-sender"),
    store: z.string().optional(),
    idleMinutes: z.number().default(120),
    mainKey: z.string().default("main"),
    agentToAgent: z
      .object({
        maxPingPongTurns: z.number().default(5),
      })
      .default({}),
  })
  .default({});

const CronJob = z.object({
  id: z.string(),
  enabled: z.boolean().default(true),
  name: z.string().optional(),
  schedule: z.string(),
  agentId: z.string().optional(),
  sessionKey: z.string().optional(),
  model: z.string().optional(),
  messageTemplate: z.string().optional(),
  timeoutSeconds: z.number().default(300),
});

const CronConfig = z
  .object({
    enabled: z.boolean().default(true),
    jobs: z.array(CronJob).default([]),
  })
  .default({});

const ProviderModel = z.object({
  id: z.string(),
  name: z.string(),
  reasoning: z.boolean().default(false),
  input: z.array(z.enum(["text", "image"])).default(["text"]),
  cost: z.object({
    input: z.number(),
    output: z.number(),
    cacheRead: z.number().default(0),
    cacheWrite: z.number().default(0),
  }),
  contextWindow: z.number(),
  maxTokens: z.number(),
});

const ProviderConfig = z.object({
  baseUrl: z.string(),
  apiKey: z.string().optional(),
  auth: z
    .enum(["api-key", "oauth", "token"])
    .default("api-key"),
  api: z
    .enum([
      "anthropic-messages",
      "openai-completions",
      "google-generative-ai",
    ])
    .default("anthropic-messages"),
  models: z.array(ProviderModel).default([]),
});

const ModelsConfig = z
  .object({
    providers: z.record(z.string(), ProviderConfig).default({}),
  })
  .default({});

// Env supports flat format { PATH: "..." } and structured { vars: { PATH: "..." } }
const EnvConfig = z.preprocess(
  (val) => {
    if (val && typeof val === "object" && !Array.isArray(val)) {
      const obj = val as Record<string, unknown>;
      if (!("vars" in obj)) {
        return { vars: obj };
      }
    }
    return val;
  },
  z.object({
    vars: z.record(z.string(), z.string()).default({}),
  }),
).default({ vars: {} });

const WatchdogService = z.object({
  name: z.string(),
  url: z.string(),
  timeoutMs: z.number().default(3000),
});

const WatchdogConfig = z
  .object({
    enabled: z.boolean().default(true),
    intervalMs: z.number().default(5 * 60 * 1000),
    alertRecipient: z.string().optional(),
    services: z.array(WatchdogService).default([]),
  })
  .default({});

// passthrough() preserves unknown top-level fields (meta, wizard, browser, tools, etc.)
// so they survive round-tripping without silent data loss
export const AletheiaConfigSchema = z.object({
  agents: AgentsConfig.default({}),
  bindings: z.array(Binding).default([]),
  channels: ChannelsConfig.default({}),
  gateway: GatewayConfig.default({}),
  plugins: PluginsConfig.default({}),
  session: SessionConfig.default({}),
  cron: CronConfig.default({}),
  models: ModelsConfig.default({}),
  env: EnvConfig.default({}),
  watchdog: WatchdogConfig.default({}),
}).passthrough();

export type AletheiaConfig = z.infer<typeof AletheiaConfigSchema>;
export type NousConfig = z.infer<typeof NousDefinition>;
export type BindingConfig = z.infer<typeof Binding>;
export type SignalAccount = z.infer<typeof SignalAccountConfig>;
