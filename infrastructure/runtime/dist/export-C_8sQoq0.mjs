import { c as paths, u as createLogger } from "./entry.mjs";
import { extname, join, relative } from "node:path";
import { existsSync, readFileSync, readdirSync } from "node:fs";

//#region src/portability/export.ts
const log = createLogger("portability:export");
const DEFAULT_OPTIONS = {
	includeMemory: false,
	includeGraph: false,
	maxMessagesPerSession: 500,
	includeArchived: false,
	sidecarUrl: "http://localhost:8230"
};
const TEXT_EXTENSIONS = new Set([
	".md",
	".txt",
	".yaml",
	".yml",
	".json",
	".ts",
	".js",
	".py",
	".sh",
	".bash",
	".zsh",
	".css",
	".html",
	".svelte",
	".toml",
	".ini",
	".cfg",
	".conf",
	".env",
	".log",
	".csv",
	".xml",
	".gitignore",
	".editorconfig",
	".prettierrc"
]);
const BINARY_EXTENSIONS = new Set([
	".png",
	".jpg",
	".jpeg",
	".gif",
	".ico",
	".svg",
	".webp",
	".woff",
	".woff2",
	".ttf",
	".eot",
	".zip",
	".tar",
	".gz",
	".bz2",
	".xz",
	".pdf",
	".doc",
	".docx",
	".xlsx",
	".db",
	".sqlite",
	".sqlite3",
	".wasm",
	".so",
	".dylib"
]);
function isTextFile(filename) {
	const ext = extname(filename).toLowerCase();
	if (TEXT_EXTENSIONS.has(ext)) return true;
	if (BINARY_EXTENSIONS.has(ext)) return false;
	if (!ext) return true;
	return false;
}
const IGNORE_DIRS = new Set([
	"node_modules",
	".git",
	"__pycache__",
	".cache",
	"dist"
]);
const MAX_FILE_SIZE = 1024 * 1024;
function scanWorkspace(workspacePath) {
	const files = {};
	const binaryFiles = [];
	function walk(dir) {
		let dirents;
		try {
			dirents = readdirSync(dir, { withFileTypes: true });
		} catch {
			return;
		}
		for (const dirent of dirents) {
			if (dirent.name.startsWith(".") && dirent.name !== ".env") continue;
			if (IGNORE_DIRS.has(dirent.name)) continue;
			const fullPath = join(dir, dirent.name);
			if (dirent.isDirectory()) {
				walk(fullPath);
				continue;
			}
			if (!dirent.isFile()) continue;
			const relPath = relative(workspacePath, fullPath);
			if (isTextFile(dirent.name)) try {
				const data = readFileSync(fullPath, "utf-8");
				if (Buffer.byteLength(data) <= MAX_FILE_SIZE) files[relPath] = data;
				else binaryFiles.push(relPath);
			} catch {
				binaryFiles.push(relPath);
			}
			else binaryFiles.push(relPath);
		}
	}
	walk(workspacePath);
	return {
		files,
		binaryFiles
	};
}
function exportSession(store, session, _nousId, maxMessages) {
	const limit = maxMessages > 0 ? maxMessages : 0;
	const messages = store.getHistory(session.id, limit > 0 ? { limit } : {});
	const notes = store.getNotes(session.id, { limit: 100 });
	return {
		id: session.id,
		sessionKey: session.sessionKey,
		status: session.status,
		sessionType: session.sessionType,
		messageCount: session.messageCount,
		tokenCountEstimate: session.tokenCountEstimate,
		distillationCount: session.distillationCount,
		createdAt: session.createdAt,
		updatedAt: session.updatedAt,
		workingState: session.workingState,
		distillationPriming: session.distillationPriming,
		notes: notes.map((n) => ({
			category: n.category,
			content: n.content,
			createdAt: n.createdAt
		})),
		messages: messages.map((m) => ({
			role: m.role,
			content: m.content,
			seq: m.seq,
			tokenEstimate: m.tokenEstimate,
			isDistilled: m.isDistilled,
			createdAt: m.createdAt
		}))
	};
}
async function exportMemoryVectors(sidecarUrl, nousId) {
	try {
		const response = await fetch(`${sidecarUrl}/memories?agent_id=${encodeURIComponent(nousId)}&limit=10000`);
		if (!response.ok) {
			log.warn(`Memory export failed: ${response.status} ${response.statusText}`);
			return [];
		}
		const data = await response.json();
		if (!data.ok || !data.memories) return [];
		return data.memories.map((m) => ({
			id: String(m["id"] ?? ""),
			text: String(m["memory"] ?? m["text"] ?? ""),
			metadata: m["metadata"] ?? {}
		}));
	} catch (error) {
		log.warn(`Memory sidecar unreachable: ${error instanceof Error ? error.message : error}`);
		return [];
	}
}
async function exportGraph(sidecarUrl) {
	try {
		const response = await fetch(`${sidecarUrl}/graph/export?mode=all`);
		if (!response.ok) {
			log.warn(`Graph export failed: ${response.status} ${response.statusText}`);
			return null;
		}
		const data = await response.json();
		if (!data.ok) return null;
		return {
			nodes: (data.nodes ?? []).map((n) => ({
				name: String(n["id"] ?? n["name"] ?? ""),
				labels: n["labels"] ?? [],
				properties: {
					pagerank: n["pagerank"],
					community: n["community"]
				}
			})),
			edges: (data.edges ?? []).map((e) => ({
				source: String(e["source"] ?? ""),
				target: String(e["target"] ?? ""),
				relType: String(e["rel_type"] ?? e["relType"] ?? "RELATED_TO")
			}))
		};
	} catch (error) {
		log.warn(`Graph export failed: ${error instanceof Error ? error.message : error}`);
		return null;
	}
}
async function exportAgent(nousId, nousConfig, store, opts) {
	const options = {
		...DEFAULT_OPTIONS,
		...opts
	};
	log.info(`Exporting agent ${nousId}...`);
	const workspacePath = paths.nousDir(nousId);
	let workspace = {
		files: {},
		binaryFiles: []
	};
	if (existsSync(workspacePath)) {
		workspace = scanWorkspace(workspacePath);
		log.info(`Workspace: ${Object.keys(workspace.files).length} text files, ${workspace.binaryFiles.length} binary files`);
	} else log.warn(`Workspace not found: ${workspacePath}`);
	const allSessions = store.listSessions(nousId);
	const sessions = options.includeArchived ? allSessions : allSessions.filter((s) => s.status !== "archived");
	const exportedSessions = sessions.map((s) => exportSession(store, s, nousId, options.maxMessagesPerSession));
	log.info(`Sessions: ${exportedSessions.length} (${sessions.length} total, ${allSessions.length - sessions.length} archived skipped)`);
	let memory;
	if (options.includeMemory || options.includeGraph) {
		memory = {};
		if (options.includeMemory) {
			memory.vectors = await exportMemoryVectors(options.sidecarUrl, nousId);
			log.info(`Memory vectors: ${memory.vectors.length}`);
		}
		if (options.includeGraph) {
			const graph = await exportGraph(options.sidecarUrl);
			if (graph) {
				memory.graph = graph;
				log.info(`Graph: ${graph.nodes.length} nodes, ${graph.edges.length} edges`);
			}
		}
	}
	const agentFile = {
		version: 1,
		exportedAt: (/* @__PURE__ */ new Date()).toISOString(),
		generator: `aletheia-export/1.0`,
		nous: {
			id: nousId,
			name: nousConfig["name"] ?? null,
			model: nousConfig["model"] ?? null,
			config: nousConfig
		},
		workspace,
		sessions: exportedSessions
	};
	if (memory) agentFile.memory = memory;
	const jsonSize = JSON.stringify(agentFile).length;
	log.info(`Export complete: ${(jsonSize / 1024 / 1024).toFixed(1)}MB`);
	return agentFile;
}
function agentFileToJson(agentFile, pretty = true) {
	return JSON.stringify(agentFile, null, pretty ? 2 : void 0);
}

//#endregion
export { agentFileToJson, exportAgent };
//# sourceMappingURL=export-C_8sQoq0.mjs.map