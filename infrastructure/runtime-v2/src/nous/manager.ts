// Nous manager — lifecycle, routing, agent turn execution
import { createLogger } from "../koina/logger.js";
import { SessionStore, type Message } from "../mneme/store.js";
import { ProviderRouter } from "../hermeneus/router.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import { ToolRegistry, type ToolContext } from "../organon/registry.js";
import { assembleBootstrap } from "./bootstrap.js";
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
    );

    log.info(
      `Processing message for ${nousId}:${sessionKey} (session ${session.id})`,
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
      session.id,
      this.config.agents.defaults.contextTokens - bootstrap.totalTokens - 8000,
    );

    const seq = this.store.appendMessage(session.id, "user", msg.text, {
      tokenEstimate: estimateTokens(msg.text),
    });

    const messages = this.buildMessages(history, msg.text);
    const toolDefs = this.tools.getDefinitions({
      allow: nous.tools.allow.length > 0 ? nous.tools.allow : undefined,
      deny: nous.tools.deny.length > 0 ? nous.tools.deny : undefined,
    });

    const toolContext: ToolContext = {
      nousId,
      sessionId: session.id,
      workspace,
    };

    if (this.plugins) {
      await this.plugins.dispatchBeforeTurn({
        nousId,
        sessionId: session.id,
        messageText: msg.text,
      });
    }

    let totalToolCalls = 0;
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
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

      this.store.recordUsage({
        sessionId: session.id,
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

      if (toolUses.length === 0 || result.stopReason === "end_turn") {
        const text = result.content
          .filter((b): b is { type: "text"; text: string } => b.type === "text")
          .map((b) => b.text)
          .join("\n");

        this.store.appendMessage(session.id, "assistant", text, {
          tokenEstimate: estimateTokens(text),
        });

        const outcome: TurnOutcome = {
          text,
          nousId,
          sessionId: session.id,
          toolCalls: totalToolCalls,
          inputTokens: totalInputTokens,
          outputTokens: totalOutputTokens,
        };

        if (this.plugins) {
          await this.plugins.dispatchAfterTurn({
            nousId,
            sessionId: session.id,
            responseText: text,
            toolCalls: totalToolCalls,
            inputTokens: totalInputTokens,
            outputTokens: totalOutputTokens,
          });
        }

        return outcome;
      }

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

        this.store.appendMessage(session.id, "tool_result", toolResult, {
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

    for (const msg of history) {
      if (msg.role === "user" || msg.role === "assistant") {
        messages.push({
          role: msg.role,
          content: msg.content,
        });
      }
    }

    messages.push({
      role: "user",
      content: currentText,
    });

    return messages;
  }
}
