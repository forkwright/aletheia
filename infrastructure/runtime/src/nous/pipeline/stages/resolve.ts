// Resolve stage — route to nous, create/find session, select model
import { createLogger } from "../../../koina/logger.js";
import {
  resolveNous,
  resolveModel,
  resolveWorkspace,
  resolveDefaultNous,
} from "../../../taxis/loader.js";
import {
  scoreComplexity,
  selectModel,
  selectTemperature,
  type ComplexityTier,
} from "../../../hermeneus/complexity.js";
import { paths } from "../../../taxis/paths.js";
import type { ToolContext } from "../../../organon/registry.js";
import type { InboundMessage, RuntimeServices, TurnState, SystemBlock } from "../types.js";
import { TraceBuilder } from "../../trace.js";
import { LoopDetector } from "../../loop-detector.js";

const log = createLogger("pipeline:resolve");

export function resolveNousId(msg: InboundMessage, services: RuntimeServices): string {
  if (msg.nousId) return msg.nousId;

  if (msg.channel && msg.peerKind && msg.peerId) {
    const routed = services.store.resolveRoute(
      msg.channel, msg.peerKind, msg.peerId, msg.accountId,
    );
    if (routed) return routed;
  }

  const defaultNous = resolveDefaultNous(services.config);
  return defaultNous?.id ?? "syn";
}

export function resolveStage(
  msg: InboundMessage,
  services: RuntimeServices,
  abortSignal?: AbortSignal,
): TurnState | false {
  const nousId = resolveNousId(msg, services);
  const nous = resolveNous(services.config, nousId);
  if (!nous) {
    log.error(`Unknown nous: ${nousId}`);
    return false;
  }

  const sessionKey = msg.sessionKey ?? "main";
  let model = msg.model ?? resolveModel(services.config, nous);

  let temperature: number | undefined;
  const routing = services.config.agents.defaults.routing;
  if (routing.enabled && !msg.model) {
    const session = services.store.findSession(nousId, sessionKey);
    const override = routing.agentOverrides[nousId] as ComplexityTier | undefined;
    const complexity = scoreComplexity({
      messageText: msg.text,
      messageCount: session?.messageCount ?? 0,
      depth: msg.depth ?? 0,
      ...(override ? { agentOverride: override } : {}),
    });
    model = selectModel(complexity.tier, routing.tiers);
    temperature = selectTemperature(complexity.tier, services.tools.hasTools());
    log.info(
      `Routing ${nousId}: ${complexity.tier} (score=${complexity.score}, ${complexity.reason}) → ${model} temp=${temperature}`,
    );
  }

  const session = services.store.findOrCreateSession(nousId, sessionKey, model, msg.parentSessionId);
  const workspace = resolveWorkspace(services.config, nous);

  // Merge per-agent allowedRoots + global defaults + ALETHEIA_ROOT
  const globalRoots = services.config.agents.defaults.allowedRoots ?? [];
  const agentRoots = nous.allowedRoots ?? [];
  const allowedRoots = [...new Set([paths.root, ...globalRoots, ...agentRoots])];

  const toolContext: ToolContext = {
    nousId,
    sessionId: session.id,
    workspace,
    allowedRoots,
    depth: msg.depth ?? 0,
    ...(abortSignal ? { signal: abortSignal } : {}),
  };

  const trace = new TraceBuilder(session.id, nousId, 0, model);

  return {
    msg,
    nousId,
    sessionId: session.id,
    sessionKey,
    model,
    nous,
    workspace,
    ...(temperature !== undefined ? { temperature } : {}),
    seq: 0,
    systemPrompt: [] as SystemBlock[],
    messages: [],
    toolDefs: [],
    toolContext,
    trace,
    totalToolCalls: 0,
    totalInputTokens: 0,
    totalOutputTokens: 0,
    totalCacheReadTokens: 0,
    totalCacheWriteTokens: 0,
    currentMessages: [],
    turnToolCalls: [],
    loopDetector: new LoopDetector(),
    ...(abortSignal ? { abortSignal } : {}),
  };
}
