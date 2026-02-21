// Custom commands — user-defined slash commands via Markdown files with YAML frontmatter
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { extname, join } from "node:path";
import { createLogger } from "../koina/logger.js";
import type { CommandHandler, CommandRegistry } from "../semeion/commands.js";
import type { NousManager } from "../nous/manager.js";

const log = createLogger("custom-commands");

export interface CommandArgument {
  name: string;
  required?: boolean;
  default?: string;
}

export interface CustomCommandDef {
  name: string;
  description: string;
  arguments: CommandArgument[];
  allowedTools?: string[];
  prompt: string;
}

interface Frontmatter {
  name?: string;
  description?: string;
  arguments?: Array<{ name: string; required?: boolean; default?: string }>;
  allowed_tools?: string[];
}

export function parseFrontmatter(content: string): { frontmatter: Frontmatter | null; body: string } {
  const lines = content.split("\n");
  if (lines[0]?.trim() !== "---") return { frontmatter: null, body: content };

  let endIdx = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i]?.trim() === "---") {
      endIdx = i;
      break;
    }
  }
  if (endIdx === -1) return { frontmatter: null, body: content };

  const yamlBlock = lines.slice(1, endIdx).join("\n");
  const body = lines.slice(endIdx + 1).join("\n").trim();

  const fm: Frontmatter = {};
  let currentKey = "";
  let inArray = false;
  let currentArrayItem: Record<string, string | boolean> = {};

  for (const line of yamlBlock.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    if (!line.startsWith(" ") && !line.startsWith("\t") && !line.startsWith("-")) {
      if (inArray && currentKey === "arguments" && Object.keys(currentArrayItem).length > 0) {
        if (!fm.arguments) fm.arguments = [];
        fm.arguments.push(currentArrayItem as { name: string; required?: boolean; default?: string });
        currentArrayItem = {};
      }
      inArray = false;

      const colonIdx = trimmed.indexOf(":");
      if (colonIdx === -1) continue;
      const key = trimmed.slice(0, colonIdx).trim();
      const value = trimmed.slice(colonIdx + 1).trim();

      if (key === "name") fm.name = value;
      else if (key === "description") fm.description = value;
      else if (key === "arguments") { currentKey = "arguments"; inArray = true; }
      else if (key === "allowed_tools") {
        const match = value.match(/\[([^\]]*)\]/);
        if (match) fm.allowed_tools = match[1]!.split(",").map((s) => s.trim());
      }
    } else if (inArray && currentKey === "arguments") {
      if (trimmed.startsWith("- ")) {
        if (Object.keys(currentArrayItem).length > 0) {
          if (!fm.arguments) fm.arguments = [];
          fm.arguments.push(currentArrayItem as { name: string; required?: boolean; default?: string });
          currentArrayItem = {};
        }
        const rest = trimmed.slice(2).trim();
        const colonIdx = rest.indexOf(":");
        if (colonIdx !== -1) {
          const k = rest.slice(0, colonIdx).trim();
          const v = rest.slice(colonIdx + 1).trim();
          if (k === "name") currentArrayItem["name"] = v;
          else if (k === "required") currentArrayItem["required"] = v === "true";
          else if (k === "default") currentArrayItem["default"] = v;
        }
      } else {
        const colonIdx = trimmed.indexOf(":");
        if (colonIdx !== -1) {
          const k = trimmed.slice(0, colonIdx).trim();
          const v = trimmed.slice(colonIdx + 1).trim();
          if (k === "name") currentArrayItem["name"] = v;
          else if (k === "required") currentArrayItem["required"] = v === "true";
          else if (k === "default") currentArrayItem["default"] = v;
        }
      }
    }
  }

  if (inArray && currentKey === "arguments" && Object.keys(currentArrayItem).length > 0) {
    if (!fm.arguments) fm.arguments = [];
    fm.arguments.push(currentArrayItem as { name: string; required?: boolean; default?: string });
  }

  return { frontmatter: fm, body };
}

export function loadCustomCommands(dir: string): CustomCommandDef[] {
  if (!existsSync(dir)) {
    log.debug(`Custom commands directory not found: ${dir}`);
    return [];
  }

  let entries: string[];
  try {
    entries = readdirSync(dir, { withFileTypes: true })
      .filter((d) => d.isFile() && extname(d.name) === ".md")
      .map((d) => d.name);
  } catch (err) {
    log.warn(`Failed to read commands directory ${dir}: ${err instanceof Error ? err.message : err}`);
    return [];
  }

  const commands: CustomCommandDef[] = [];

  for (const filename of entries) {
    const filePath = join(dir, filename);
    try {
      const content = readFileSync(filePath, "utf-8");
      const { frontmatter, body } = parseFrontmatter(content);

      if (!frontmatter?.name || !frontmatter?.description) {
        log.warn(`Skipping ${filename}: missing name or description in frontmatter`);
        continue;
      }

      commands.push({
        name: frontmatter.name,
        description: frontmatter.description,
        arguments: (frontmatter.arguments ?? []).map((a) => ({
          name: a.name,
          ...(a.required !== undefined ? { required: a.required } : {}),
          ...(a.default !== undefined ? { default: a.default } : {}),
        })),
        ...(frontmatter.allowed_tools ? { allowedTools: frontmatter.allowed_tools } : {}),
        prompt: body,
      });
    } catch (err) {
      log.warn(`Failed to load command ${filename}: ${err instanceof Error ? err.message : err}`);
    }
  }

  log.info(`Loaded ${commands.length} custom commands from ${dir}`);
  return commands;
}

export function substituteArgs(prompt: string, argDefs: CommandArgument[], argsStr: string): { prompt: string; error?: string } {
  const parts = argsStr.trim().split(/\s+/).filter(Boolean);
  const values: Record<string, string> = {};

  for (let i = 0; i < argDefs.length; i++) {
    const def = argDefs[i]!;
    if (i < parts.length) {
      values[def.name] = parts[i]!;
    } else if (def.default !== undefined) {
      values[def.name] = def.default;
    } else if (def.required) {
      const usage = argDefs.map((a) => a.required ? `<${a.name}>` : `[${a.name}]`).join(" ");
      return { prompt: "", error: `Missing required argument: ${def.name}\n**Usage:** /${argDefs[0]?.name ? "" : ""}${usage}` };
    }
  }

  let result = prompt;
  for (const [name, value] of Object.entries(values)) {
    result = result.replaceAll(`$${name}`, value);
  }
  return { prompt: result };
}

export function registerCustomCommands(
  defs: CustomCommandDef[],
  registry: CommandRegistry,
  manager: NousManager,
): number {
  let count = 0;
  for (const def of defs) {
    const handler: CommandHandler = {
      name: def.name,
      description: def.description,
      async execute(args, ctx) {
        const { prompt, error } = substituteArgs(def.prompt, def.arguments, args);
        if (error) return error;

        const sessionKey = ctx.sessionId
          ? undefined
          : `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;

        try {
          const outcome = await manager.handleMessage({
            text: prompt,
            channel: ctx.isGroup ? "signal-group" : "signal",
            ...(sessionKey ? { sessionKey } : {}),
            ...(ctx.sessionId ? { sessionKey: ctx.sessionId } : {}),
            ...(def.allowedTools ? { toolFilter: def.allowedTools } : {}),
          });
          return outcome.text;
        } catch (err) {
          return `Command failed: ${err instanceof Error ? err.message : String(err)}`;
        }
      },
    };
    registry.register(handler);
    count++;
  }
  return count;
}
