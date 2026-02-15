// Cross-agent deliberation — structured multi-turn dialectic protocol
import { createLogger } from "../../koina/logger.js";
import type { ToolHandler, ToolContext } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";

const log = createLogger("organon.deliberate");

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

const PHASES = ["pose", "critique", "revise", "synthesize"] as const;
type Phase = (typeof PHASES)[number];

const PHASE_PROMPTS: Record<Phase, (topic: string, prior: string) => string> = {
  pose: (topic) =>
    `[DELIBERATION — POSE PHASE]\n` +
    `Topic: ${topic}\n\n` +
    `Present your informed position on this topic. Be specific and substantive. ` +
    `State your reasoning, key assumptions, and any caveats. ` +
    `This will be reviewed by other agents — make your strongest case.`,

  critique: (topic, prior) =>
    `[DELIBERATION — CRITIQUE PHASE]\n` +
    `Topic: ${topic}\n\n` +
    `Another agent posed this position:\n${prior}\n\n` +
    `Critically evaluate this position. Identify:\n` +
    `- Strengths worth preserving\n` +
    `- Weaknesses, gaps, or blind spots\n` +
    `- Missing perspectives or considerations\n` +
    `- Alternative approaches worth exploring\n` +
    `Be constructive but honest. Disagree where warranted.`,

  revise: (topic, prior) =>
    `[DELIBERATION — REVISE PHASE]\n` +
    `Topic: ${topic}\n\n` +
    `Your original position received this critique:\n${prior}\n\n` +
    `Revise your position in light of this feedback. ` +
    `Incorporate valid criticisms. Defend points where you believe the critique is wrong. ` +
    `Produce a stronger, more nuanced position.`,

  synthesize: (topic, prior) =>
    `[DELIBERATION — SYNTHESIS PHASE]\n` +
    `Topic: ${topic}\n\n` +
    `Deliberation transcript:\n${prior}\n\n` +
    `Synthesize the key findings from this deliberation:\n` +
    `1. Points of agreement\n` +
    `2. Unresolved tensions\n` +
    `3. Recommended action or conclusion\n` +
    `4. Open questions for future consideration\n` +
    `Be concise and actionable.`,
};

export function createDeliberateTool(
  dispatcher?: AgentDispatcher,
): ToolHandler {
  return {
    category: "available",
    definition: {
      name: "deliberate",
      description:
        "Start a structured multi-agent deliberation on a topic.\n\n" +
        "USE WHEN:\n" +
        "- Facing a decision with multiple valid approaches\n" +
        "- Uncertainty is high and you want diverse perspectives\n" +
        "- Cross-domain question that benefits from specialist input\n" +
        "- You suspect your own view may be biased or incomplete\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Simple factual question — use sessions_ask instead\n" +
        "- You already know what to do and just need help executing\n" +
        "- Time-critical action where deliberation would delay too long\n\n" +
        "TIPS:\n" +
        "- Protocol: pose → critique → revise → synthesize\n" +
        "- Choose 1-2 agents whose domains are relevant to the topic\n" +
        "- Results are posted to the blackboard for all agents to see\n" +
        "- Each phase builds on the previous — this is a genuine dialectic",
      input_schema: {
        type: "object",
        properties: {
          topic: {
            type: "string",
            description: "The topic or question to deliberate on",
          },
          agents: {
            type: "array",
            items: { type: "string" },
            description:
              "Agent IDs to participate (1-3 agents, e.g. ['eiron', 'arbor'])",
          },
          timeoutPerPhase: {
            type: "number",
            description: "Max seconds per phase (default: 90)",
          },
        },
        required: ["topic", "agents"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const topic = input["topic"] as string;
      const agents = input["agents"] as string[];
      const timeoutPerPhase = (input["timeoutPerPhase"] as number) ?? 90;

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      if (!agents || agents.length === 0 || agents.length > 3) {
        return JSON.stringify({
          error: "Provide 1-3 agent IDs to deliberate with",
        });
      }

      // Remove self from agents list if present
      const participants = agents.filter((a) => a !== context.nousId);
      if (participants.length === 0) {
        return JSON.stringify({
          error: "Need at least one other agent to deliberate with",
        });
      }

      const deliberationId = `delib_${Date.now().toString(36)}`;
      const sessionKey = `deliberation:${deliberationId}`;
      const poser = participants[0]!;
      const critic =
        participants.length > 1 ? participants[1]! : context.nousId;
      const synthesizer =
        participants.length > 2
          ? participants[2]!
          : participants.length > 1
            ? poser
            : context.nousId;

      const transcript: Array<{
        phase: Phase;
        agent: string;
        content: string;
      }> = [];
      const tokens = { input: 0, output: 0 };

      log.info(
        `Starting deliberation ${deliberationId}: "${topic.slice(0, 80)}" with [${participants.join(", ")}]`,
      );

      try {
        // PHASE 1: POSE — first participant presents their position
        const poseResult = await askAgent(
          dispatcher,
          poser,
          PHASE_PROMPTS.pose(topic, ""),
          sessionKey,
          context,
          timeoutPerPhase,
        );

        if (poseResult.error) {
          return JSON.stringify({
            deliberationId,
            error: `Pose phase failed: ${poseResult.error}`,
            phase: "pose",
          });
        }

        transcript.push({
          phase: "pose",
          agent: poser,
          content: poseResult.text,
        });
        tokens.input += poseResult.tokens.input;
        tokens.output += poseResult.tokens.output;

        // PHASE 2: CRITIQUE — second participant (or caller) critiques
        let critiqueResult: AskResult;

        if (critic === context.nousId) {
          // Caller does the critique — return partial result for them to critique
          critiqueResult = {
            text: "[Caller acts as critic — critique provided by initiating agent]",
            tokens: { input: 0, output: 0 },
          };
        } else {
          critiqueResult = await askAgent(
            dispatcher,
            critic,
            PHASE_PROMPTS.critique(topic, poseResult.text),
            sessionKey,
            context,
            timeoutPerPhase,
          );

          if (critiqueResult.error) {
            // Continue with what we have — critique failure is non-fatal
            critiqueResult = {
              text: `[Critique phase skipped: ${critiqueResult.error}]`,
              tokens: { input: 0, output: 0 },
            };
          }
        }

        transcript.push({
          phase: "critique",
          agent: critic,
          content: critiqueResult.text,
        });
        tokens.input += critiqueResult.tokens.input;
        tokens.output += critiqueResult.tokens.output;

        // PHASE 3: REVISE — original poser revises in light of critique
        const reviseResult = await askAgent(
          dispatcher,
          poser,
          PHASE_PROMPTS.revise(topic, critiqueResult.text),
          sessionKey,
          context,
          timeoutPerPhase,
        );

        if (reviseResult.error) {
          transcript.push({
            phase: "revise",
            agent: poser,
            content: `[Revision skipped: ${reviseResult.error}]`,
          });
        } else {
          transcript.push({
            phase: "revise",
            agent: poser,
            content: reviseResult.text,
          });
          tokens.input += reviseResult.tokens.input;
          tokens.output += reviseResult.tokens.output;
        }

        // PHASE 4: SYNTHESIZE — use a third agent or fall back to poser
        const fullTranscript = transcript
          .map(
            (t) => `[${t.phase.toUpperCase()} — ${t.agent}]\n${t.content}`,
          )
          .join("\n\n---\n\n");

        let synthesisText: string;

        if (synthesizer === context.nousId) {
          synthesisText =
            "[Synthesis returned to caller — see transcript below]";
        } else {
          const synthResult = await askAgent(
            dispatcher,
            synthesizer,
            PHASE_PROMPTS.synthesize(topic, fullTranscript),
            sessionKey,
            context,
            timeoutPerPhase,
          );
          synthesisText = synthResult.error
            ? `[Synthesis failed: ${synthResult.error}]`
            : synthResult.text;
          tokens.input += synthResult.tokens.input;
          tokens.output += synthResult.tokens.output;
        }

        transcript.push({
          phase: "synthesize",
          agent: synthesizer,
          content: synthesisText,
        });

        // Post to blackboard for all agents to see
        if (dispatcher.store) {
          const summary = `DELIBERATION: ${topic.slice(0, 100)}\n\n${synthesisText.slice(0, 1500)}`;
          dispatcher.store.blackboardWrite(
            `deliberation:${deliberationId}`,
            summary,
            context.nousId,
            7200,
          );

          // Record deliberation audit trail
          dispatcher.store.recordCrossAgentCall({
            sourceSessionId: context.sessionId,
            sourceNousId: context.nousId,
            targetNousId: participants.join(","),
            kind: "ask",
            content: `[deliberation:${deliberationId}] ${topic.slice(0, 500)}`,
          });
        }

        log.info(
          `Deliberation ${deliberationId} complete: ${transcript.length} phases, ` +
            `${tokens.input + tokens.output} total tokens`,
        );

        return JSON.stringify({
          deliberationId,
          topic,
          participants: [context.nousId, ...participants],
          phases: transcript.map((t) => ({
            phase: t.phase,
            agent: t.agent,
            content: t.content,
          })),
          synthesis: synthesisText,
          tokens,
        });
      } catch (err) {
        log.error(`Deliberation ${deliberationId} failed: ${err}`);
        return JSON.stringify({
          deliberationId,
          error: err instanceof Error ? err.message : String(err),
          phases_completed: transcript.map((t) => t.phase),
          partial_transcript: transcript,
        });
      }
    },
  };
}

interface AskResult {
  text: string;
  error?: string;
  tokens: { input: number; output: number };
}

async function askAgent(
  dispatcher: AgentDispatcher,
  agentId: string,
  message: string,
  sessionKey: string,
  context: ToolContext,
  timeoutSeconds: number,
): Promise<AskResult> {
  let timer: ReturnType<typeof setTimeout>;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`Timeout after ${timeoutSeconds}s`)),
      timeoutSeconds * 1000,
    );
  });

  try {
    const outcome = await Promise.race([
      dispatcher.handleMessage({
        text: message,
        nousId: agentId,
        sessionKey,
        parentSessionId: context.sessionId,
        channel: "internal",
        peerKind: "agent",
        peerId: context.nousId,
        depth: (context.depth ?? 0) + 1,
      }),
      timeoutPromise,
    ]);
    clearTimeout(timer!);

    return {
      text: outcome.text,
      tokens: { input: outcome.inputTokens, output: outcome.outputTokens },
    };
  } catch (err) {
    clearTimeout(timer!);
    return {
      text: "",
      error: err instanceof Error ? err.message : String(err),
      tokens: { input: 0, output: 0 },
    };
  }
}

export const deliberateTool = createDeliberateTool();
