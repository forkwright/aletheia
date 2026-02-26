// Skills directory — loads SKILL.md files and exposes them for bootstrap/commands
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("organon:skills");

export interface SkillDefinition {
  id: string;
  name: string;
  description: string;
  instructions: string;
  tools?: string[];
  domains?: string[];
}

export class SkillRegistry {
  private skills = new Map<string, SkillDefinition>();

  loadFromDirectory(dir: string): void {
    if (!existsSync(dir)) {
      log.debug(`Skills directory not found: ${dir}`);
      return;
    }

    let entries: string[];
    try {
      entries = readdirSync(dir, { withFileTypes: true })
        .filter((d) => d.isDirectory())
        .map((d) => d.name);
    } catch (error) {
      log.warn(`Failed to read skills directory ${dir}: ${error instanceof Error ? error.message : error}`);
      return;
    }

    for (const entry of entries) {
      const skillPath = join(dir, entry, "SKILL.md");
      if (!existsSync(skillPath)) continue;

      try {
        const content = readFileSync(skillPath, "utf-8");
        const skill = parseSkillMd(entry, content);
        if (skill) {
          this.skills.set(skill.id, skill);
          log.debug(`Loaded skill: ${skill.id} — ${skill.name}`);
        }
      } catch (error) {
        log.warn(`Failed to load skill ${entry}: ${error instanceof Error ? error.message : error}`);
      }
    }

    log.info(`Loaded ${this.skills.size} skills from ${dir}`);
  }

  get(id: string): SkillDefinition | undefined {
    return this.skills.get(id);
  }

  listAll(): SkillDefinition[] {
    return [...this.skills.values()];
  }

  addSkill(id: string, definition: SkillDefinition): void {
    this.skills.set(id, definition);
    log.info(`Added skill: ${id} — ${definition.name}`);
  }

  get size(): number {
    return this.skills.size;
  }

  getSkillsForDomain(domain: string): SkillDefinition[] {
    return [...this.skills.values()].filter((s) => s.domains?.includes(domain));
  }

  toBootstrapSection(): string {
    if (this.skills.size === 0) return "";
    const lines = ["## Available Skills", ""];
    for (const skill of this.skills.values()) {
      lines.push(`- **${skill.name}**: ${skill.description}`);
    }
    return lines.join("\n");
  }
}

function parseYamlInlineArray(value: string): string[] {
  const trimmed = value.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) return [];
  return trimmed
    .slice(1, -1)
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

function parseFrontmatter(raw: string): { meta: Record<string, string[]>; body: string } {
  if (!raw.startsWith("---\n")) return { meta: {}, body: raw };
  const endIdx = raw.indexOf("\n---\n", 4);
  if (endIdx === -1) return { meta: {}, body: raw };

  const fmBlock = raw.slice(4, endIdx);
  const body = raw.slice(endIdx + 5);
  const meta: Record<string, string[]> = {};

  for (const line of fmBlock.split("\n")) {
    const colonIdx = line.indexOf(":");
    if (colonIdx === -1) continue;
    const key = line.slice(0, colonIdx).trim();
    const value = line.slice(colonIdx + 1).trim();
    meta[key] = parseYamlInlineArray(value);
  }

  return { meta, body };
}

function parseSkillMd(id: string, content: string): SkillDefinition | null {
  const { meta, body } = parseFrontmatter(content);
  const lines = body.split("\n");

  // Extract title from first heading
  const titleLine = lines.find((l) => l.startsWith("# "));
  if (!titleLine) return null;
  const name = titleLine.replace(/^#+\s*/, "").trim();

  // Extract first paragraph as description
  let description = "";
  const titleIdx = lines.indexOf(titleLine);
  for (let i = titleIdx + 1; i < lines.length; i++) {
    const line = lines[i]!.trim();
    if (line === "") continue;
    if (line.startsWith("#")) break;
    description = line;
    break;
  }

  return {
    id,
    name,
    description,
    instructions: body,
    ...(meta["tools"] && meta["tools"].length > 0 ? { tools: meta["tools"] } : {}),
    ...(meta["domains"] && meta["domains"].length > 0 ? { domains: meta["domains"] } : {}),
  };
}
