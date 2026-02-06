/**
 * Patch for OpenClaw workspace.js — Dynamic context support
 * 
 * If a workspace contains CONTEXT.md, load ONLY that file (plus SOUL.md)
 * instead of all 7 default workspace files. This lets Aletheia's
 * compile-context generate a single optimized context payload.
 *
 * Apply: node /mnt/ssd/aletheia/infrastructure/patches/apply-workspace-patch.js
 * Revert: node /mnt/ssd/aletheia/infrastructure/patches/apply-workspace-patch.js --revert
 */

// The patched version of loadWorkspaceBootstrapFiles
export const PATCHED_FUNCTION = `
export async function loadWorkspaceBootstrapFiles(dir) {
    const resolvedDir = resolveUserPath(dir);
    
    // ALETHEIA PATCH: Check for compiled context first
    const compiledContextPath = path.join(resolvedDir, "CONTEXT.md");
    try {
        await fs.access(compiledContextPath);
        // Compiled context exists — use it as primary, plus SOUL.md only
        const entries = [
            { name: "SOUL.md", filePath: path.join(resolvedDir, DEFAULT_SOUL_FILENAME) },
            { name: "CONTEXT.md", filePath: compiledContextPath },
        ];
        // Still load MEMORY.md if present
        entries.push(...(await resolveMemoryBootstrapEntries(resolvedDir)));
        
        const result = [];
        for (const entry of entries) {
            try {
                const content = await fs.readFile(entry.filePath, "utf-8");
                result.push({ name: entry.name, path: entry.filePath, content, missing: false });
            } catch {
                result.push({ name: entry.name, path: entry.filePath, missing: true });
            }
        }
        return result;
    } catch {
        // No compiled context — fall through to default behavior
    }
    // END ALETHEIA PATCH
    
    const entries = [
        { name: DEFAULT_AGENTS_FILENAME, filePath: path.join(resolvedDir, DEFAULT_AGENTS_FILENAME) },
        { name: DEFAULT_SOUL_FILENAME, filePath: path.join(resolvedDir, DEFAULT_SOUL_FILENAME) },
        { name: DEFAULT_TOOLS_FILENAME, filePath: path.join(resolvedDir, DEFAULT_TOOLS_FILENAME) },
        { name: DEFAULT_IDENTITY_FILENAME, filePath: path.join(resolvedDir, DEFAULT_IDENTITY_FILENAME) },
        { name: DEFAULT_USER_FILENAME, filePath: path.join(resolvedDir, DEFAULT_USER_FILENAME) },
        { name: DEFAULT_HEARTBEAT_FILENAME, filePath: path.join(resolvedDir, DEFAULT_HEARTBEAT_FILENAME) },
        { name: DEFAULT_BOOTSTRAP_FILENAME, filePath: path.join(resolvedDir, DEFAULT_BOOTSTRAP_FILENAME) },
    ];
    entries.push(...(await resolveMemoryBootstrapEntries(resolvedDir)));
    const result = [];
    for (const entry of entries) {
        try {
            const content = await fs.readFile(entry.filePath, "utf-8");
            result.push({ name: entry.name, path: entry.filePath, content, missing: false });
        } catch {
            result.push({ name: entry.name, path: entry.filePath, missing: true });
        }
    }
    return result;
}`;
