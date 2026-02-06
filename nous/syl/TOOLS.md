# TOOLS.md - Syl's Tools

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/aletheia/shared/TOOLS-INFRASTRUCTURE.md) for common commands (gcal, gdrive, tw, letta, pplx, facts, mcporter).


## Google Calendar

Check and manage Cooper's schedule and family events:

```bash
# List calendars
./bin/gcal calendars

# Cooper's schedule (Family calendar)
./bin/gcal events -c "family13408790289857991137@group.calendar.google.com" -d 3

# Today's family events
./bin/gcal today -c "family13408790289857991137@group.calendar.google.com"

# Add timed event
./bin/gcal add "Event title" -c "family13408790289857991137@group.calendar.google.com" -s "2026-01-29T14:00" -e "2026-01-29T15:00"

# Add all-day event
./bin/gcal add "Event" -c "family13408790289857991137@group.calendar.google.com" -s "2026-01-29" --all-day
```

**Calendar IDs:**
- Family (Cooper's schedule): `family13408790289857991137@group.calendar.google.com` ✅
- Cody's work: `ckickertz@summusglobal.com` ✅
- Kendall's work: `kendall-work` (via ical) ✅

## Kendall's Work Calendar (Outlook)

```bash
./bin/ical events kendall-work --days 3
```

Note: Shows free/busy status only (not event titles) due to Outlook privacy settings. ✅

## Perplexity (Research)

```bash
./bin/pplx "your question here"
./bin/pplx "your question" --sources  # include source URLs
```

Great for fact-checking, research, current events.

## Grocery List Management

```bash
./bin/grocery list                           # show current list
./bin/grocery add "milk" -q 2 -c dairy       # add item with quantity/category
./bin/grocery remove 1                       # remove item by index
./bin/grocery clear                          # clear entire list
```

For managing HEB shopping lists as needed by Kendall/Cody.

## PDF Reading

```bash
pdftotext /path/to/file.pdf -     # extract text to stdout
pdftotext /path/to/file.pdf       # extract to text file
```

Built-in capability for reading PDF documents.

## Task Management

```bash
./bin/tw                                     # show next actions
./bin/tw add "task" project:family priority:M due:2026-02-01
./bin/tw done 1                              # complete task #1
./bin/tw list                                # all tasks  
./bin/tw today                               # due today
./bin/tw week                                # due this week
```

Projects: `family`, `cody`, `kendall`, `household`. Priorities: H/M/L.

## Getting Help

If you need access to something else or hit a problem, reach out to Syn:

```
sessions_send with sessionKey "agent:main:main" and your message
```

---

*Updated: 2026-01-28*

## Task Management

**Namespace:** `project:home`

```bash
# Add home task
tw add "description" project:home priority:M

# Subprojects
tw add "..." project:home.calendar    # Family calendar items
tw add "..." project:home.errands     # Shopping, pickups
tw add "..." project:home.maintenance # House maintenance

# View home tasks
tw project:home
tw project:home +urgent
```

**Tags:** +errand, +appointment, +kendall, +family, +blocked, +review

## Letta Memory

Agent: syl-memory (agent-9aa39693-3bbe-44ae-afb6-041d37ac45a2)

```bash
# Check status (auto-detects agent from workspace)
letta status

# Store a fact
letta remember "important fact here"

# Query memory
letta ask "what do you know about X?"

# Search archival memory
letta recall "topic"

# View memory blocks
letta blocks

# Use explicit agent
letta --agent syl status
```
