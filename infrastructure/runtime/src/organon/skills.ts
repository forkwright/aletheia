// Skills directory — loads SKILL.md files and exposes them for bootstrap/commands
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("organon:skills");

export interface SkillDefinition {
  id: string;
  name: string;
  description: string;
  instructions: string;
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
    } catch (err) {
      log.warn(`Failed to read skills directory ${dir}: ${err instanceof Error ? err.message : err}`);
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
      } catch (err) {
        log.warn(`Failed to load skill ${entry}: ${err instanceof Error ? err.message : err}`);
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

  toBootstrapSection(): string {
    if (this.skills.size === 0) return "";
    const lines = ["## Available Skills", ""];
    for (const skill of this.skills.values()) {
      lines.push(`- **${skill.name}**: ${skill.description}`);
    }
    return lines.join("\n");
  }
}

function parseSkillMd(id: string, content: string): SkillDefinition | null {
  const lines = content.split("\n");

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
    instructions: content,
  };
}
