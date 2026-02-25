// workspace_index tool — manage and query the workspace file index
import { existsSync } from "node:fs";
import type { ToolHandler, ToolContext } from "../registry.js";
import {
  loadIndexConfig,
  saveIndexConfig,
  indexWorkspace,
  rebuildWorkspaceIndex,
  queryIndex,
} from "../workspace-indexer.js";

export function createWorkspaceIndexTool(): ToolHandler {
  return {
    definition: {
      name: "workspace_index",
      description:
        "Manage and query the workspace file index.\n\n" +
        "USE WHEN:\n" +
        "- You need to find files matching a query without running ls/find\n" +
        "- You want to add an extra directory to the index (e.g. a data directory you use often)\n" +
        "- You want to force a full index rebuild after bulk changes\n\n" +
        "ACTIONS:\n" +
        "- status: show index stats (file count, last built, configured extra paths)\n" +
        "- add_path: add an absolute directory path to the persistent index config\n" +
        "- remove_path: remove a path from the index config\n" +
        "- rebuild: force a full re-scan of workspace and all extra paths",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["status", "add_path", "remove_path", "rebuild"],
            description: "Action to perform",
          },
          path: {
            type: "string",
            description: "Absolute path to add or remove (required for add_path/remove_path)",
          },
          query: {
            type: "string",
            description: "Optional keyword query to run after rebuild",
          },
        },
        required: ["action"],
      },
    },
    category: "available",
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const action = input["action"] as string;
      const workspace = context.workspace;

      if (!workspace) {
        return JSON.stringify({ error: "No workspace configured" });
      }

      const cfg = await loadIndexConfig(workspace);

      if (action === "status") {
        const index = await indexWorkspace(workspace, cfg.extraPaths);
        const builtAt = new Date(index.builtAt).toISOString();
        return JSON.stringify({
          workspace,
          fileCount: index.files.length,
          builtAt,
          extraPaths: cfg.extraPaths,
        }, null, 2);
      }

      if (action === "add_path") {
        const path = input["path"] as string | undefined;
        if (!path) return JSON.stringify({ error: "path is required for add_path" });
        if (!existsSync(path)) return JSON.stringify({ error: `Path does not exist: ${path}` });
        if (cfg.extraPaths.includes(path)) {
          return JSON.stringify({ message: "Path already in index config", path });
        }
        cfg.extraPaths.push(path);
        await saveIndexConfig(workspace, cfg);
        await rebuildWorkspaceIndex(workspace, cfg.extraPaths);
        return JSON.stringify({ added: path, extraPaths: cfg.extraPaths });
      }

      if (action === "remove_path") {
        const path = input["path"] as string | undefined;
        if (!path) return JSON.stringify({ error: "path is required for remove_path" });
        const before = cfg.extraPaths.length;
        cfg.extraPaths = cfg.extraPaths.filter((p) => p !== path);
        if (cfg.extraPaths.length === before) {
          return JSON.stringify({ message: "Path not found in config", path });
        }
        await saveIndexConfig(workspace, cfg);
        return JSON.stringify({ removed: path, extraPaths: cfg.extraPaths });
      }

      if (action === "rebuild") {
        const index = await rebuildWorkspaceIndex(workspace, cfg.extraPaths);
        const result: Record<string, unknown> = {
          rebuilt: true,
          fileCount: index.files.length,
          extraPaths: cfg.extraPaths,
        };
        const query = input["query"] as string | undefined;
        if (query) {
          const hits = queryIndex(index, query, 10);
          result["queryResults"] = hits.map((f) => ({ path: f.path, firstLine: f.firstLine }));
        }
        return JSON.stringify(result, null, 2);
      }

      return JSON.stringify({ error: `Unknown action: ${action}` });
    },
  };
}
