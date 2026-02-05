# Self-Audit Routine

*Run weekly (Sunday heartbeat) or when workspace feels bloated.*

## Quick Checks

### 1. File Count
```bash
find /mnt/ssd/moltbot/clawd -type f | wc -l
```
Alert if >100 files. Investigate growth.

### 2. Memory Files
```bash
ls memory/*.md | wc -l
```
If >14 daily files: consolidate old ones into MEMORY.md, archive raw files.

### 3. Reviews Folder
```bash
ls reviews/*.md | wc -l
```
If >20: archive old PR reviews (keep last 2 weeks).

### 4. Empty Folders
Delete or use. No zombies.

### 5. Context Folder
Should only contain active reference docs. If something's internalized → delete the file.

## Consolidation Process

**Weekly:**
1. Read past week's daily memory files
2. Extract anything significant into MEMORY.md
3. Move raw daily files older than 2 weeks to `memory/archive/`

**Monthly:**
1. Review MEMORY.md for stale info
2. Review TOOLS.md for outdated notes
3. Check Letta for contradictions with current state
4. Delete anything redundant

## Red Flags

- Same info in 3+ places → consolidate
- File not touched in 30 days → archive or delete
- Empty folder → delete or document purpose
- Context file I've already internalized → delete

---

*Created: 2026-01-29*
