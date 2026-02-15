// Nous manager — lifecycle, routing, agent turn execution
import { createLogger } from "../koina/logger.js";
import { SessionStore, type Message } from "../mneme/store.js";
import { ProviderRouter } from "../hermeneus/router.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import { ToolRegistry, type ToolContext } from "../organon/registry.js";
import { assembleBootstrap } from "./bootstrap.js";
import { shouldDistill, distillSession } from "../distillation/pipeline.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import {
  resolveNous,
  resolveModel,
  resolveWorkspace,
  resolveDefaultNous,
} from "../taxis/loader.js";
import type {
  ContentBlock,
  MessageParam,
  ToolUseBlock,
  UserContentBlock,
} from "../hermeneus/anthropic.js";
import type { PluginRegistry } from "../prostheke/registry.js";

const log = createLogger("nous");

export interface InboundMessage {
  text: string;
  nousId?: string;
  sessionKey?: string;
  parentSessionId?: string;
  channel?: string;
  peerId?: string;
  peerKind?: string;
  accountId?: string;
  mediaUrls?: string[];
}

export interface TurnOutcome {
  text: string;
  nousId: string;
  sessionId: string;
  toolCalls: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

// Per-session mutex to prevent concurrent turns from corrupting context
const sessionLocks = new Map<string, Promise<unknown>>();

function withSessionLock<T>(sessionId: string, fn: () => Promise<T>): Promise<T> {
  const previous = sessionLocks.get(sessionId) ?? Promise.resolve();
  const current = previous.then(fn, fn);
  sessionLocks.set(sessionId, current);
  // Suppress unhandled rejection from the .finally() chain — the caller handles the original
  current.finally(() => {
    if (sessionLocks.get(sessionId) === current) {
      sessionLocks.delete(sessionId);
    }
  }).catch(() => {});
  return current;
}

export class NousManager {
  private plugins?: PluginRegistry;

  constructor(
    private config: AletheiaConfig,
    private store: SessionStore,
    private router: ProviderRouter,
    private tools: ToolRegistry,
  ) {
    log.info(
      `NousManager initialized with ${config.agents.list.length} nous`,
    );
  }

  setPlugins(plugins: PluginRegistry): void {
    this.plugins = plugins;
  }

  async handleMessage(msg: InboundMessage): Promise<TurnOutcome> {
    const nousId = this.resolveNousId(msg);
    const nous = resolveNous(this.config, nousId);
    if (!nous) {
      throw new Error(`Unknown nous: ${nousId}`);
    }

    const sessionKey = msg.sessionKey ?? "main";
    const model = resolveModel(this.config, nous);
    const session = this.store.findOrCreateSession(
      nousId,
      sessionKey,
      model,
      msg.parentSessionId,
    );

    // Serialize concurrent turns on the same session
    return withSessionLock(session.id, () =>
      this.executeTurn(nousId, session.id, sessionKey, model, msg, nous),
    );
  }

  private async executeTurn(
    nousId: string,
    sessionId: string,
    sessionKey: string,
    model: string,
    msg: InboundMessage,
    nous: ReturnType<typeof resolveNous>,
  ): Promise<TurnOutcome> {
    if (!nous) throw new Error(`Unknown nous: ${nousId}`);

    log.info(
      `Processing message for ${nousId}:${sessionKey} (session ${sessionId})`,
    );

    const workspace = resolveWorkspace(this.config, nous);
    const bootstrap = assembleBootstrap(workspace, {
      maxTokens: this.config.agents.defaults.bootstrapMaxChars,
    });

    const systemPrompt = [
      ...bootstrap.staticBlocks,
      ...bootstrap.dynamicBlocks,
    ];

    const history = this.store.getHistoryWithBudget(
      sessionId,
      this.config.agents.defaults.contextTokens - bootstrap.totalTokens - 8000,
    );

    const seq = this.store.appendMessage(sessionId, "user", msg.text, {
      tokenEstimate: estimateTokens(msg.text),
    });

    const messages = this.buildMessages(history, msg.text);
    const toolDefs = this.tools.getDefinitions({
      allow: nous.tools.allow.length > 0 ? nous.tools.allow : undefined,
      deny: nous.tools.deny.length > 0 ? nous.tools.deny : undefined,
    });

    const toolContext: ToolContext = {
      nousId,
      sessionId,
      workspace,
    };

    if (this.plugins) {
      await this.plugins.dispatchBeforeTurn({
        nousId,
        sessionId,
        messageText: msg.text,
      });
    }

    let totalToolCalls = 0;
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let totalCacheReadTokens = 0;
    let totalCacheWriteTokens = 0;
    let currentMessages = messages;

    const MAX_TOOL_LOOPS = 20;
    for (let loop = 0; loop < MAX_TOOL_LOOPS; loop++) {
      const result = await this.router.complete({
        model,
        system: systemPrompt,
        messages: currentMessages,
        tools: toolDefs.length > 0 ? toolDefs : undefined,
        maxTokens: 8192,
      });

      totalInputTokens += result.usage.inputTokens;
      totalOutputTokens += result.usage.outputTokens;
      totalCacheReadTokens += result.usage.cacheReadTokens;
      totalCacheWriteTokens += result.usage.cacheWriteTokens;

      this.store.recordUsage({
        sessionId,
        turnSeq: seq + loop,
        inputTokens: result.usage.inputTokens,
        outputTokens: result.usage.outputTokens,
        cacheReadTokens: result.usage.cacheReadTokens,
        cacheWriteTokens: result.usage.cacheWriteTokens,
        model: result.model,
      });

      const toolUses = result.content.filter(
        (b): b is ToolUseBlock => b.type === "tool_use",
      );

      // Only exit when there are no tool calls — don't check stopReason
      // (Anthropic can return end_turn with tool_use blocks in the same response)
      if (toolUses.length === 0) {
        const text = result.content
          .filter((b): b is { type: "text"; text: string } => b.type === "text")
          .map((b) => b.text)
          .join("\n");

        this.store.appendMessage(sessionId, "assistant", text, {
          tokenEstimate: estimateTokens(text),
        });

        const outcome: TurnOutcome = {
          text,
          nousId,
          sessionId,
          toolCalls: totalToolCalls,
          inputTokens: totalInputTokens,
          outputTokens: totalOutputTokens,
          cacheReadTokens: totalCacheReadTokens,
          cacheWriteTokens: totalCacheWriteTokens,
        };

        const cacheHitRate = totalInputTokens > 0
          ? Math.round((totalCacheReadTokens / totalInputTokens) * 100)
          : 0;
        log.info(
          `Turn complete for ${nousId}: ${totalInputTokens}in/${totalOutputTokens}out, ` +
          `cache ${totalCacheReadTokens}r/${totalCacheWriteTokens}w (${cacheHitRate}% hit), ` +
          `${totalToolCalls} tool calls`,
        );

        if (this.plugins) {
          await this.plugins.dispatchAfterTurn({
            nousId,
            sessionId,
            responseText: text,
            messageText: msg.text,
            toolCalls: totalToolCalls,
            inputTokens: totalInputTokens,
            outputTokens: totalOutputTokens,
          });
        }

        // Auto-trigger distillation when context grows too large
        const contextTokens = this.config.agents.defaults.contextTokens;
        const threshold = Math.floor(contextTokens * 0.65);
        try {
          if (await shouldDistill(this.store, sessionId, { threshold, minMessages: 10 })) {
            log.info(`Distillation triggered for session ${sessionId}`);
            await distillSession(this.store, this.router, sessionId, nousId, {
              triggerThreshold: threshold,
              minMessages: 10,
              extractionModel: "claude-haiku-4-5-20251001",
              summaryModel: "claude-haiku-4-5-20251001",
              plugins: this.plugins,
            });
          }
        } catch (err) {
          log.warn(`Distillation failed: ${err instanceof Error ? err.message : err}`);
        }

        return outcome;
      }

      // Store the assistant's tool_use response as JSON for history replay
      this.store.appendMessage(
        sessionId,
        "assistant",
        JSON.stringify(result.content),
        { tokenEstimate: estimateTokens(JSON.stringify(result.content)) },
      );

      currentMessages = [
        ...currentMessages,
        {
          role: "assistant" as const,
          content: result.content as ContentBlock[],
        },
      ];

      const toolResults: UserContentBlock[] = [];
      for (const toolUse of toolUses) {
        totalToolCalls++;
        log.debug(`Tool call: ${toolUse.name}`);

        let toolResult: string;
        let isError = false;
        try {
          toolResult = await this.tools.execute(
            toolUse.name,
            toolUse.input,
            toolContext,
          );
        } catch (err) {
          isError = true;
          toolResult = err instanceof Error ? err.message : String(err);
          log.warn(`Tool ${toolUse.name} failed: ${toolResult}`);
        }

        toolResults.push({
          type: "tool_result",
          tool_use_id: toolUse.id,
          content: toolResult,
          is_error: isError || undefined,
        });

        this.store.appendMessage(sessionId, "tool_result", toolResult, {
          toolCallId: toolUse.id,
          toolName: toolUse.name,
          tokenEstimate: estimateTokens(toolResult),
        });
      }

      currentMessages = [
        ...currentMessages,
        {
          role: "user" as const,
          content: toolResults,
        },
      ];
    }

    throw new Error("Max tool loops exceeded");
  }

  private resolveNousId(msg: InboundMessage): string {
    if (msg.nousId) return msg.nousId;

    if (msg.channel && msg.peerKind && msg.peerId) {
      const routed = this.store.resolveRoute(
        msg.channel,
        msg.peerKind,
        msg.peerId,
        msg.accountId,
      );
      if (routed) return routed;
    }

    const defaultNous = resolveDefaultNous(this.config);
    return defaultNous?.id ?? "syn";
  }

  private buildMessages(
    history: Message[],
    currentText: string,
  ): MessageParam[] {
    const messages: MessageParam[] = [];

    for (let i = 0; i < history.length; i++) {
      const msg = history[i]!;

      if (msg.role === "user") {
        messages.push({ role: "user", content: msg.content });
      } else if (msg.role === "assistant") {
        // Try parsing as JSON content blocks (tool_use responses stored as JSON)
        try {
          const parsed = JSON.parse(msg.content);
          if (Array.isArray(parsed) && parsed.length > 0 && parsed[0]?.type) {
            messages.push({
              role: "assistant",
              content: parsed as ContentBlock[],
            });
            continue;
          }
        } catch {
          // Not JSON — plain text assistant message
        }
        messages.push({ role: "assistant", content: msg.content });
      } else if (msg.role === "tool_result") {
        // Group consecutive tool_results into a single user message
        const toolResults: UserContentBlock[] = [];
        while (i < history.length && history[i]!.role === "tool_result") {
          const tr = history[i]!;
          toolResults.push({
            type: "tool_result",
            tool_use_id: tr.toolCallId ?? "",
            content: tr.content,
          });
          i++;
        }
        i--; // Back up — for loop will increment

        // Validate: tool_results must follow an assistant message with matching tool_use blocks.
        // Old runtime stored tool_results without the preceding assistant tool_use — skip orphans.
        const prev = messages[messages.length - 1];
        if (prev?.role === "assistant" && Array.isArray(prev.content)) {
          const toolUseIds = new Set(
            (prev.content as ContentBlock[])
              .filter((b): b is ToolUseBlock => b.type === "tool_use")
              .map((b) => b.id),
          );
          const valid = toolResults.filter((tr) =>
            "tool_use_id" in tr && toolUseIds.has(tr.tool_use_id),
          );
          if (valid.length > 0) {
            messages.push({ role: "user", content: valid });
          } else {
            log.debug("Dropping orphaned tool_results (no matching tool_use)");
          }
        } else {
          log.debug("Dropping orphaned tool_results (no preceding assistant tool_use)");
        }
      }
    }

    messages.push({
      role: "user",
      content: currentText,
    });

    return messages;
  }
}
