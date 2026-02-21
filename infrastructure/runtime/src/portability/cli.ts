// CLI entry point for agent export/import
//
// Usage:
//   npx tsx src/portability/cli.ts export <nous-id> [options]
//
// Options:
//   --output, -o    Output file path (default: <nous-id>-<date>.agent.json)
//   --memory        Include memory vectors
//   --graph         Include knowledge graph
//   --archived      Include archived sessions
//   --compact       Compact JSON (no pretty-printing)
//   --sidecar-url   Memory sidecar URL (default: http://localhost:8230)

import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { agentFileToJson, exportAgent } from "./export.js";
import { loadConfig } from "../taxis/loader.js";
import { SessionStore } from "../mneme/store.js";
import { paths } from "../taxis/paths.js";

async function main() {
  const args = process.argv.slice(2);

  if (args.length === 0 || args[0] === "--help" || args[0] === "-h") {
    console.log(`
Usage: aletheia-export <nous-id> [options]

Options:
  --output, -o <file>   Output file (default: <nous-id>-<date>.agent.json)
  --memory              Include memory vectors from Qdrant
  --graph               Include knowledge graph from Neo4j
  --archived            Include archived sessions
  --compact             Compact JSON output
  --sidecar-url <url>   Memory sidecar URL (default: http://localhost:8230)
  --help, -h            Show this help
`);
    process.exit(0);
  }

  const command = args[0];
  if (command !== "export") {
    console.error(`Unknown command: ${command}. Currently only 'export' is supported.`);
    process.exit(1);
  }

  const nousId = args[1];
  if (!nousId) {
    console.error("Error: nous-id is required");
    process.exit(1);
  }

  // Parse options
  let output = "";
  let includeMemory = false;
  let includeGraph = false;
  let includeArchived = false;
  let compact = false;
  let sidecarUrl = "http://localhost:8230";

  for (let i = 2; i < args.length; i++) {
    switch (args[i]) {
      case "--output":
      case "-o":
        output = args[++i] ?? "";
        break;
      case "--memory":
        includeMemory = true;
        break;
      case "--graph":
        includeGraph = true;
        break;
      case "--archived":
        includeArchived = true;
        break;
      case "--compact":
        compact = true;
        break;
      case "--sidecar-url":
        sidecarUrl = args[++i] ?? sidecarUrl;
        break;
    }
  }

  // Default output filename
  if (!output) {
    const date = new Date().toISOString().split("T")[0];
    output = `${nousId}-${date}.agent.json`;
  }

  // Load config to find the agent
  const config = loadConfig();
  const agentDef = config.agents.list.find((a) => a.id === nousId);

  if (!agentDef) {
    console.error(`Error: Agent '${nousId}' not found in config.`);
    console.error(`Available agents: ${config.agents.list.map((a) => a.id).join(", ")}`);
    process.exit(1);
  }

  // Open session store
  const store = new SessionStore(paths.sessionsDb());

  try {
    console.log(`Exporting agent: ${nousId}`);

    const agentFile = await exportAgent(nousId, agentDef as unknown as Record<string, unknown>, store, {
      includeMemory,
      includeGraph,
      includeArchived,
      sidecarUrl,
    });

    const json = agentFileToJson(agentFile, !compact);
    const outputPath = resolve(output);
    writeFileSync(outputPath, json);

    const sizeMb = (json.length / 1024 / 1024).toFixed(1);
    console.log(`\nExported to: ${outputPath}`);
    console.log(`Size: ${sizeMb}MB`);
    console.log(`Files: ${Object.keys(agentFile.workspace.files).length} text, ${agentFile.workspace.binaryFiles.length} binary`);
    console.log(`Sessions: ${agentFile.sessions.length}`);
    if (agentFile.memory?.vectors) {
      console.log(`Memory vectors: ${agentFile.memory.vectors.length}`);
    }
    if (agentFile.memory?.graph) {
      console.log(`Graph: ${agentFile.memory.graph.nodes.length} nodes, ${agentFile.memory.graph.edges.length} edges`);
    }
  } finally {
    store.close();
  }
}

main().catch((err) => {
  console.error("Export failed:", err);
  process.exit(1);
});
