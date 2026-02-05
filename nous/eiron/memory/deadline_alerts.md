# Deadline Alert System

**Created:** 2026-02-02
**Purpose:** Never let Cody miss a deadline

## How It Works

1. **Cron job** runs daily at 6pm CST
   - Location: `/mnt/ssd/moltbot/clawd/bin/eiron-deadline-check`
   - Triggers Eiron to check Todoist

2. **When triggered**, I should:
   - Check Todoist TEMBA project for tasks due tomorrow
   - If any exist: Message Cody with summary
   - If none: Log and skip (no message)

3. **Message format:**
   ```
   📚 Tomorrow's deadlines:
   - [Task name] @ [time]
   - [Task name] @ [time]
   ```

## Data Sources

- **Primary:** Todoist (TEMBA project via MCP)
- **Backup:** Google Calendar (school calendar)
- **Reference:** MBA syllabus files in `/mnt/ssd/moltbot/clawd/mba/sp26/`

## Log Location

`/mnt/ssd/moltbot/eiron/deadline-check.log`

## Manual Check

If cron fails, can manually run:
```bash
/mnt/ssd/moltbot/clawd/bin/eiron-deadline-check
```

---

*This is the ONE job that cannot be missed.*
