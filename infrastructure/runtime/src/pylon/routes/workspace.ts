// Workspace file explorer routes
import { Hono } from "hono";
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, rmdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { execSync } from "node:child_process";
import { createLogger } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

interface TreeEntry {
  name: string;
  type: "file" | "directory";
  size?: number | undefined;
  modified?: string | undefined;
  children?: TreeEntry[] | undefined;
}

function resolveAgentWorkspace(config: RouteDeps["config"], agentId?: string): string | null {
  const id = agentId ?? config.agents.list.find((a) => a.default)?.id ?? config.agents.list[0]?.id;
  if (!id) return null;
  const agent = config.agents.list.find((a) => a.id === id);
  return agent?.workspace ?? null;
}

function safeWorkspacePath(workspace: string, userPath: string): string | null {
  const resolved = resolve(workspace, userPath);
  if (!resolved.startsWith(workspace)) return null;
  return resolved;
}

function buildTree(dirPath: string, depth: number, maxDepth: number): TreeEntry[] {
  if (depth >= maxDepth) return [];
  try {
    const entries = readdirSync(dirPath, { withFileTypes: true });
    const result: TreeEntry[] = [];
    for (const entry of entries) {
      if (entry.name.startsWith(".")) continue;
      const fullPath = join(dirPath, entry.name);
      try {
        const stat = statSync(fullPath);
        if (entry.isDirectory()) {
          result.push({
            name: entry.name,
            type: "directory",
            modified: stat.mtime.toISOString(),
            children: depth + 1 < maxDepth ? buildTree(fullPath, depth + 1, maxDepth) : undefined,
          });
        } else {
          result.push({
            name: entry.name,
            type: "file",
            size: stat.size,
            modified: stat.mtime.toISOString(),
          });
        }
      } catch (error) {
        log.debug(`Skipping unreadable entry ${entry.name}: ${error instanceof Error ? error.message : error}`);
      }
    }
    result.sort((a, b) => {
      if (a.type !== b.type) return a.type === "directory" ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    return result;
  } catch (error) {
    log.debug(`buildTree failed for directory: ${error instanceof Error ? error.message : error}`);
    return [];
  }
}

export function workspaceRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { config } = deps;

  app.get("/api/workspace/tree", (c) => {
    const agentId = c.req.query("agentId");
    const subpath = c.req.query("path") ?? "";
    const depth = Math.min(parseInt(c.req.query("depth") ?? "2", 10), 5);
    const workspace = resolveAgentWorkspace(config, agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const targetPath = subpath ? safeWorkspacePath(workspace, subpath) : workspace;
    if (!targetPath) return c.json({ error: "Invalid path" }, 400);
    if (!existsSync(targetPath)) return c.json({ error: "Path not found" }, 404);

    const tree = buildTree(targetPath, 0, depth);
    return c.json({ root: subpath || ".", entries: tree });
  });

  app.get("/api/workspace/file", (c) => {
    const agentId = c.req.query("agentId");
    const filePath = c.req.query("path");
    if (!filePath) return c.json({ error: "path required" }, 400);

    const workspace = resolveAgentWorkspace(config, agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolved = safeWorkspacePath(workspace, filePath);
    if (!resolved) return c.json({ error: "Invalid path" }, 400);

    try {
      const stat = statSync(resolved);
      if (stat.isDirectory()) return c.json({ error: "Path is a directory" }, 400);
      if (stat.size > 1_048_576) return c.json({ error: "File too large (>1MB)" }, 400);

      const content = readFileSync(resolved, "utf-8");
      return c.json({ path: filePath, size: stat.size, content });
    } catch (error) {
      if (error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT") {
        return c.json({ error: "File not found" }, 404);
      }
      return c.json({ error: error instanceof Error ? error.message : "Read failed" }, 500);
    }
  });

  app.put("/api/workspace/file", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }

    const filePath = body["path"] as string;
    const content = body["content"];
    const agentId = body["agentId"] as string | undefined;

    if (!filePath || typeof content !== "string") {
      return c.json({ error: "path and content required" }, 400);
    }

    const workspace = resolveAgentWorkspace(config, agentId);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolved = safeWorkspacePath(workspace, filePath);
    if (!resolved) return c.json({ error: "Invalid path" }, 400);

    try {
      writeFileSync(resolved, content, "utf-8");
      return c.json({ ok: true, path: filePath, size: Buffer.byteLength(content) });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Workspace file write failed: ${msg}`);
      return c.json({ error: msg }, 500);
    }
  });

  app.get("/api/workspace/git-status", (c) => {
    const agentId = c.req.query("agentId");
    const workspace = resolveAgentWorkspace(config, agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    try {
      const output = execSync("git status --porcelain 2>/dev/null || true", {
        cwd: workspace,
        encoding: "utf-8",
        timeout: 5000,
      });
      const files: Array<{ status: string; path: string }> = [];
      for (const line of output.split("\n")) {
        if (!line.trim()) continue;
        const status = line.slice(0, 2).trim();
        const path = line.slice(3);
        files.push({ status, path });
      }
      return c.json({ files });
    } catch (error) {
      log.debug(`git-status failed: ${error instanceof Error ? error.message : error}`);
      return c.json({ files: [] });
    }
  });

  // DELETE /api/workspace/file — delete a single file or empty directory
  app.delete("/api/workspace/file", (c) => {
    const filePath = c.req.query("path");
    const agentId = c.req.query("agentId");
    if (!filePath) return c.json({ error: "path required" }, 400);

    const workspace = resolveAgentWorkspace(config, agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolved = safeWorkspacePath(workspace, filePath);
    if (!resolved) return c.json({ error: "Invalid path" }, 400);

    try {
      const stat = statSync(resolved);
      if (stat.isDirectory()) {
        // Only allow deleting empty directories
        const entries = readdirSync(resolved);
        if (entries.length > 0) return c.json({ error: "Directory not empty" }, 400);
        rmdirSync(resolved);
      } else {
        unlinkSync(resolved);
      }
      return c.json({ ok: true, path: filePath });
    } catch (error) {
      if (error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT") {
        return c.json({ error: "File not found" }, 404);
      }
      return c.json({ error: error instanceof Error ? error.message : "Delete failed" }, 500);
    }
  });

  // POST /api/workspace/file/move — rename or move a file/directory
  app.post("/api/workspace/file/move", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }

    const from = body["from"] as string;
    const to = body["to"] as string;
    const agentId = body["agentId"] as string | undefined;
    if (!from || !to) return c.json({ error: "from and to required" }, 400);

    const workspace = resolveAgentWorkspace(config, agentId);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolvedFrom = safeWorkspacePath(workspace, from);
    const resolvedTo = safeWorkspacePath(workspace, to);
    if (!resolvedFrom || !resolvedTo) return c.json({ error: "Invalid path" }, 400);

    if (!existsSync(resolvedFrom)) return c.json({ error: "Source not found" }, 404);
    if (existsSync(resolvedTo)) return c.json({ error: "Destination already exists" }, 409);

    try {
      // Create parent directories if needed
      const parentDir = dirname(resolvedTo);
      if (!existsSync(parentDir)) mkdirSync(parentDir, { recursive: true });
      renameSync(resolvedFrom, resolvedTo);
      return c.json({ ok: true, from, to });
    } catch (error) {
      return c.json({ error: error instanceof Error ? error.message : "Move failed" }, 500);
    }
  });

  // GET /api/workspace/search — content search across files
  app.get("/api/workspace/search", (c) => {
    const query = c.req.query("q");
    const agentId = c.req.query("agentId");
    const glob = c.req.query("glob");
    const maxResults = Math.min(parseInt(c.req.query("maxResults") ?? "50", 10), 200);

    if (!query) return c.json({ error: "q required" }, 400);

    const workspace = resolveAgentWorkspace(config, agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    try {
      // Use ripgrep if available, else grep
      const globArg = glob ? `--glob '${glob}'` : "";
      const cmd = `rg --no-heading --line-number --max-count ${maxResults} --max-filesize 1M ${globArg} -- ${JSON.stringify(query)} . 2>/dev/null || grep -rn --max-count=${maxResults} --include='${glob || "*"}' -- ${JSON.stringify(query)} . 2>/dev/null || true`;
      const output = execSync(cmd, {
        cwd: workspace,
        encoding: "utf-8",
        timeout: 10000,
        maxBuffer: 1024 * 1024,
      });

      const results: Array<{ path: string; line: string; lineNumber: number }> = [];
      for (const rawLine of output.split("\n")) {
        if (!rawLine.trim() || results.length >= maxResults) break;
        // Format: ./path/to/file:lineNumber:content
        const match = rawLine.match(/^\.\/(.+?):(\d+):(.*)$/);
        if (match) {
          results.push({ path: match[1]!, lineNumber: parseInt(match[2]!, 10), line: match[3]!.slice(0, 200) });
        }
      }
      return c.json({ results });
    } catch (error) {
      log.debug(`workspace search failed: ${error instanceof Error ? error.message : error}`);
      return c.json({ results: [] });
    }
  });

  return app;
}
