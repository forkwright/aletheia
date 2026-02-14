# Inbox Triage Routine

*Process items from dianoia/inbox/ and capture observations*

## Sources

1. **dianoia/inbox/*.md** — dropped notes, ideas, observations
2. **Chat captures** — things Cody says worth saving
3. **System observations** — patterns, friction, improvements

## Processing Steps

### 1. Read inbox items
```bash
ls -la /mnt/ssd/aletheia/dianoia/inbox/
cat /mnt/ssd/aletheia/dianoia/inbox/*.md
```

### 2. Classify each item

| Type | Action |
|------|--------|
| **Task** | Add to taskwarrior: `tw add "..." project:X domain:X` |
| **Idea** | Log to `memory/ideas.md` or relevant domain file |
| **Observation** | Update MEMORY.md or context files |
| **Profile** | Update USER.md or cognitive profile |
| **Reference** | File to appropriate `context/` or `domains/` |

### 3. Route by domain

| Domain | Destination |
|--------|-------------|
| sophia | `domains/sophia.md`, syn-infra tasks |
| techne | `domains/techne.md`, repo issues |
| autarkeia | USER.md, career files, personal tasks |
| metaxynoesis | `domains/metaxynoesis.md`, research |

### 4. Archive processed items
```bash
# Move to dated archive
mv /mnt/ssd/aletheia/dianoia/inbox/item.md \
   /mnt/ssd/aletheia/dianoia/inbox/archive/2026-01-28/
```

## Natural Capture Triggers

Listen for these patterns in chat:
- "I was thinking..." → idea capture
- "Wouldn't it be cool if..." → feature idea
- "This is annoying..." → friction observation
- "We should..." → potential task
- Repeated corrections → implicit friction

## Output

After processing:
1. Confirm items processed
2. Note any tasks created
3. Highlight anything needing Cody's input
4. Clear inbox

---

*Template v1 - 2026-01-28*
