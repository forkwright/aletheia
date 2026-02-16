# TOOLS.md - Syl's Tools

> **Shared tools:** All tools available via exec. Shell scripts in `/mnt/ssd/aletheia/shared/bin/` are on PATH.


## Google Calendar

Check and manage Cooper's schedule and family events:

```bash
# List calendars
/mnt/ssd/aletheia/shared/bin/gcal calendars

# Cooper's schedule (Family calendar)
/mnt/ssd/aletheia/shared/bin/gcal events -c "family13408790289857991137@group.calendar.google.com" -d 3

# Today's family events
/mnt/ssd/aletheia/shared/bin/gcal today -c "family13408790289857991137@group.calendar.google.com"

# Add timed event
/mnt/ssd/aletheia/shared/bin/gcal add "Event title" -c "family13408790289857991137@group.calendar.google.com" -s "2026-01-29T14:00" -e "2026-01-29T15:00"

# Add all-day event
/mnt/ssd/aletheia/shared/bin/gcal add "Event" -c "family13408790289857991137@group.calendar.google.com" -s "2026-01-29" --all-day
```

**Calendar IDs:**
- Family (Cooper's schedule): `family13408790289857991137@group.calendar.google.com` ✅
- Cody's work: `ckickertz@summusglobal.com` ✅
- Kendall's work: `kendall-work` (via ical) ✅

## Kendall's Work Calendar (Outlook)

```bash
/mnt/ssd/aletheia/shared/bin/ical events kendall-work --days 3
```

Note: Shows free/busy status only (not event titles) due to Outlook privacy settings. ✅

## Perplexity (Research)

```bash
/mnt/ssd/aletheia/shared/bin/pplx "your question here"
/mnt/ssd/aletheia/shared/bin/pplx "your question" --sources  # include source URLs
```

Great for fact-checking, research, current events.

## Grocery List Management

```bash
/mnt/ssd/aletheia/shared/bin/grocery list                           # show current list
/mnt/ssd/aletheia/shared/bin/grocery add "milk" -q 2 -c dairy       # add item with quantity/category
/mnt/ssd/aletheia/shared/bin/grocery remove 1                       # remove item by index
/mnt/ssd/aletheia/shared/bin/grocery clear                          # clear entire list
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
/mnt/ssd/aletheia/shared/bin/tw                                     # show next actions
/mnt/ssd/aletheia/shared/bin/tw add "task" project:family priority:M due:2026-02-01
/mnt/ssd/aletheia/shared/bin/tw done 1                              # complete task #1
/mnt/ssd/aletheia/shared/bin/tw list                                # all tasks  
/mnt/ssd/aletheia/shared/bin/tw today                               # due today
/mnt/ssd/aletheia/shared/bin/tw week                                # due this week
```

Projects: `family`, `cody`, `kendall`, `household`. Priorities: H/M/L.

## Getting Help

If you need access to something else or hit a problem, reach out to Syn:

```
sessions_send with sessionKey "agent:main:main" and your message
```

---

*Updated: 2026-01-28*

## Memory

Use the `mem0_search` tool for semantic recall across extracted memories. Facts are automatically extracted from conversations.
