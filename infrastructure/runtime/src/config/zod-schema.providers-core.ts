import { z } from "zod";
import { ChannelHeartbeatVisibilitySchema } from "./zod-schema.channels.js";
import {
  BlockStreamingCoalesceSchema,
  DmConfigSchema,
  DmPolicySchema,
  ExecutableTokenSchema,
  GroupPolicySchema,
  MarkdownConfigSchema,
  requireOpenAllowFrom,
} from "./zod-schema.core.js";

export const SignalAccountSchemaBase = z
  .object({
    name: z.string().optional(),
    capabilities: z.array(z.string()).optional(),
    markdown: MarkdownConfigSchema,
    enabled: z.boolean().optional(),
    configWrites: z.boolean().optional(),
    account: z.string().optional(),
    httpUrl: z.string().optional(),
    httpHost: z.string().optional(),
    httpPort: z.number().int().positive().optional(),
    cliPath: ExecutableTokenSchema.optional(),
    autoStart: z.boolean().optional(),
    startupTimeoutMs: z.number().int().min(1000).max(120000).optional(),
    receiveMode: z.union([z.literal("on-start"), z.literal("manual")]).optional(),
    ignoreAttachments: z.boolean().optional(),
    ignoreStories: z.boolean().optional(),
    sendReadReceipts: z.boolean().optional(),
    dmPolicy: DmPolicySchema.optional().default("pairing"),
    allowFrom: z.array(z.union([z.string(), z.number()])).optional(),
    groupAllowFrom: z.array(z.union([z.string(), z.number()])).optional(),
    groupPolicy: GroupPolicySchema.optional().default("allowlist"),
    historyLimit: z.number().int().min(0).optional(),
    dmHistoryLimit: z.number().int().min(0).optional(),
    dms: z.record(z.string(), DmConfigSchema.optional()).optional(),
    textChunkLimit: z.number().int().positive().optional(),
    chunkMode: z.enum(["length", "newline"]).optional(),
    blockStreaming: z.boolean().optional(),
    blockStreamingCoalesce: BlockStreamingCoalesceSchema.optional(),
    mediaMaxMb: z.number().int().positive().optional(),
    reactionNotifications: z.enum(["off", "own", "all", "allowlist"]).optional(),
    reactionAllowlist: z.array(z.union([z.string(), z.number()])).optional(),
    actions: z
      .object({
        reactions: z.boolean().optional(),
      })
      .strict()
      .optional(),
    reactionLevel: z.enum(["off", "ack", "minimal", "extensive"]).optional(),
    heartbeat: ChannelHeartbeatVisibilitySchema,
    responsePrefix: z.string().optional(),
  })
  .strict();

export const SignalAccountSchema = SignalAccountSchemaBase.superRefine((value, ctx) => {
  requireOpenAllowFrom({
    policy: value.dmPolicy,
    allowFrom: value.allowFrom,
    ctx,
    path: ["allowFrom"],
    message: 'channels.signal.dmPolicy="open" requires channels.signal.allowFrom to include "*"',
  });
});

export const SignalConfigSchema = SignalAccountSchemaBase.extend({
  accounts: z.record(z.string(), SignalAccountSchema.optional()).optional(),
}).superRefine((value, ctx) => {
  requireOpenAllowFrom({
    policy: value.dmPolicy,
    allowFrom: value.allowFrom,
    ctx,
    path: ["allowFrom"],
    message: 'channels.signal.dmPolicy="open" requires channels.signal.allowFrom to include "*"',
  });
});
