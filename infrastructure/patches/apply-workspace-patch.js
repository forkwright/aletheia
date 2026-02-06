#!/usr/bin/env node
/**
 * Apply or revert the Aletheia workspace patch to OpenClaw.
 * 
 * Usage:
 *   node apply-workspace-patch.js          # Apply patch
 *   node apply-workspace-patch.js --revert # Revert to backup
 *   node apply-workspace-patch.js --check  # Check if patched
 */

import fs from 'node:fs';
import path from 'node:path';

const TARGET = '/usr/lib/node_modules/openclaw/dist/agents/workspace.js';
const BACKUP = TARGET + '.aletheia-backup';
const PATCH_MARKER = '// ALETHEIA PATCH';

const args = process.argv.slice(2);
const revert = args.includes('--revert');
const check = args.includes('--check');

if (check) {
    const content = fs.readFileSync(TARGET, 'utf-8');
    if (content.includes(PATCH_MARKER)) {
        console.log('PATCHED');
    } else {
        console.log('UNPATCHED');
    }
    process.exit(0);
}

if (revert) {
    if (!fs.existsSync(BACKUP)) {
        console.error('No backup found at', BACKUP);
        process.exit(1);
    }
    fs.copyFileSync(BACKUP, TARGET);
    console.log('Reverted to backup.');
    process.exit(0);
}

// Apply patch
const content = fs.readFileSync(TARGET, 'utf-8');

if (content.includes(PATCH_MARKER)) {
    console.log('Already patched.');
    process.exit(0);
}

// Backup original
fs.copyFileSync(TARGET, BACKUP);
console.log('Backed up original to', BACKUP);

// Find and replace loadWorkspaceBootstrapFiles
const funcStart = 'export async function loadWorkspaceBootstrapFiles(dir) {';
const funcStartIdx = content.indexOf(funcStart);

if (funcStartIdx === -1) {
    console.error('Could not find loadWorkspaceBootstrapFiles in', TARGET);
    process.exit(1);
}

// Find the end of the function — it ends with the closing brace before
// the SUBAGENT_BOOTSTRAP_ALLOWLIST line
const allowlistMarker = 'const SUBAGENT_BOOTSTRAP_ALLOWLIST';
const allowlistIdx = content.indexOf(allowlistMarker);

if (allowlistIdx === -1) {
    console.error('Could not find SUBAGENT_BOOTSTRAP_ALLOWLIST marker');
    process.exit(1);
}

// The function ends just before the allowlist line
// Find the last '}' before allowlistIdx
let funcEndIdx = content.lastIndexOf('}', allowlistIdx);
funcEndIdx += 1; // include the brace

const originalFunc = content.substring(funcStartIdx, funcEndIdx);

// Build replacement
const replacement = `export async function loadWorkspaceBootstrapFiles(dir) {
    const resolvedDir = resolveUserPath(dir);
    
    // ALETHEIA PATCH: Check for compiled context first
    const compiledContextPath = path.join(resolvedDir, "CONTEXT.md");
    try {
        await fs.access(compiledContextPath);
        // Compiled context exists — use it as primary, plus SOUL.md only
        const contextEntries = [
            { name: "SOUL.md", filePath: path.join(resolvedDir, DEFAULT_SOUL_FILENAME) },
            { name: "CONTEXT.md", filePath: compiledContextPath },
        ];
        // Still load MEMORY.md if present
        contextEntries.push(...(await resolveMemoryBootstrapEntries(resolvedDir)));
        
        const contextResult = [];
        for (const entry of contextEntries) {
            try {
                const content = await fs.readFile(entry.filePath, "utf-8");
                contextResult.push({ name: entry.name, path: entry.filePath, content, missing: false });
            } catch {
                contextResult.push({ name: entry.name, path: entry.filePath, missing: true });
            }
        }
        return contextResult;
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

const patched = content.substring(0, funcStartIdx) + replacement + content.substring(funcEndIdx);
fs.writeFileSync(TARGET, patched);
console.log('Patch applied successfully.');
console.log('If CONTEXT.md exists in a workspace, only SOUL.md + CONTEXT.md + MEMORY.md will be loaded.');
console.log('Otherwise, default behavior is preserved.');
