// Agent workspace scaffolding with onboarding SOUL.md
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readJson, writeJson } from "../koina/fs.js";
import type { AletheiaConfig } from "./schema.js";

export interface ScaffoldOpts {
  id: string;
  name: string;
  emoji?: string;
  nousDir: string;
  configPath: string;
  templateDir: string;
}

export interface ScaffoldResult {
  workspace: string;
  configUpdated: boolean;
  filesCreated: string[];
}

const ID_PATTERN = /^[a-z][a-z0-9-]*$/;
const RESERVED_IDS = new Set(["_example", "_onboarding", "_template"]);
const MIN_ID_LEN = 2;
const MAX_ID_LEN = 30;

const TEMPLATE_FILES = [
  "AGENTS.md", "GOALS.md", "MEMORY.md", "TOOLS.md", "CONTEXT.md", "PROSOCHE.md",
];

export function validateAgentId(id: string): { valid: boolean; reason?: string } {
  if (!id) return { valid: false, reason: "ID cannot be empty" };
  if (id.length < MIN_ID_LEN) return { valid: false, reason: `ID must be at least ${MIN_ID_LEN} characters` };
  if (id.length > MAX_ID_LEN) return { valid: false, reason: `ID must be at most ${MAX_ID_LEN} characters` };
  if (RESERVED_IDS.has(id)) return { valid: false, reason: `"${id}" is reserved` };
  if (!ID_PATTERN.test(id)) return { valid: false, reason: "ID must be lowercase alphanumeric with hyphens, starting with a letter" };
  if (id.endsWith("-")) return { valid: false, reason: "ID cannot end with a hyphen" };
  return { valid: true };
}

function onboardingSoul(name: string): string {
  return `# Onboarding

You are **${name}**. This is your first conversation with your operator.

Your goal is to learn enough about your operator and your role to write your own identity files. Until you do, these onboarding instructions will be your system prompt on every turn.

## What to Learn

Work through these topics one at a time. Don't rush — ask one question, listen, reflect back, then move on.

1. **Operator** — name, how they prefer to be addressed, communication style preferences
2. **Your domain** — what area(s) you'll focus on, what tasks you'll handle
3. **Working style** — formality level, verbosity, proactivity, when to ask vs act
4. **Boundaries** — what you should never do, what requires confirmation, uncertainty handling

## Guidelines

- One topic at a time. Reflect back what you heard before moving on.
- Reference AGENTS.md for operational defaults — don't re-ask what's already documented.
- Demonstrate calibration live: match the style they describe as you go.

## When You're Ready

After covering all topics, summarize what you've learned and ask the operator to confirm. Once confirmed, use the write tool to create these files in your workspace:

1. **SOUL.md** — your identity, voice, values, working style (this replaces this onboarding file)
2. **USER.md** — operator profile, preferences, communication style
3. **MEMORY.md** — initial knowledge base from the conversation

After writing these files, naturally transition to your first real task. Your first responses after onboarding should demonstrate the calibrated style your operator described.
`;
}

export function scaffoldAgent(opts: ScaffoldOpts): ScaffoldResult {
  const validation = validateAgentId(opts.id);
  if (!validation.valid) {
    throw new Error(`Invalid agent ID: ${validation.reason}`);
  }

  const workspace = join(opts.nousDir, opts.id);
  if (existsSync(workspace)) {
    throw new Error(`Workspace already exists: ${workspace}`);
  }

  const config = readJson<AletheiaConfig>(opts.configPath);
  if (!config) {
    throw new Error(`Cannot read config: ${opts.configPath}`);
  }

  const agents = config.agents?.list ?? [];
  if (agents.some((a: { id: string }) => a.id === opts.id)) {
    throw new Error(`Agent ID "${opts.id}" already exists in config`);
  }

  mkdirSync(workspace, { recursive: true });
  const filesCreated: string[] = [];

  for (const file of TEMPLATE_FILES) {
    const src = join(opts.templateDir, file);
    if (existsSync(src)) {
      copyFileSync(src, join(workspace, file));
      filesCreated.push(file);
    }
  }

  const identityContent = `name: ${opts.name}\nemoji: ${opts.emoji ?? "🤖"}\n`;
  writeFileSync(join(workspace, "IDENTITY.md"), identityContent, "utf-8");
  filesCreated.push("IDENTITY.md");

  writeFileSync(join(workspace, "SOUL.md"), onboardingSoul(opts.name), "utf-8");
  filesCreated.push("SOUL.md");

  writeFileSync(join(workspace, "USER.md"), "# Operator\n\n*To be written during onboarding.*\n", "utf-8");
  filesCreated.push("USER.md");

  agents.push({
    id: opts.id,
    workspace,
    name: opts.name,
    identity: { name: opts.name, emoji: opts.emoji ?? "🤖" },
  } as AletheiaConfig["agents"]["list"][number]);

  const bindings = config.bindings ?? [];
  bindings.push({
    agentId: opts.id,
    match: { channel: "web" },
  } as AletheiaConfig["bindings"][number]);

  config.agents.list = agents;
  config.bindings = bindings;
  writeJson(opts.configPath, config);

  return { workspace, configUpdated: true, filesCreated };
}
