# Morning Brief Routine

*Run daily at 8:00 AM CST via cron*

## Checklist

### 1. Weather
```bash
# Get weather for location
curl -s "wttr.in/Chicago?format=3"
```
- Current conditions
- High/low for today
- Notable alerts (rain, snow, extreme temps)

### 2. Calendar (when gog skill ready)
```bash
# gog calendar today
# gog calendar tomorrow
```
- Today's events with times
- Tomorrow preview if notable
- Conflicts or back-to-back meetings

### 3. Tasks
```bash
tw next limit:5
tw due:today
```
- Top 5 by urgency
- Any due today
- Blockers or waiting items

### 4. GitHub
```bash
gh pr list --repo forkwright/mouseion --state open --json number,title | jq length
gh pr list --repo forkwright/akroasis --state open --json number,title | jq length
```
- Open PRs needing review
- CI failures
- New issues since yesterday

### 5. Infrastructure
```bash
df -h /mnt/nas/Media | awk 'NR==2{print "NAS: "$5}'
docker ps --filter "health=unhealthy" -q | wc -l
```
- NAS disk usage (alert if >93%)
- Unhealthy containers
- Service status

### 6. Email (when himalaya ready)
```bash
# himalaya list --folder INBOX --max 10
```
- Unread count
- Urgent/important flagged

## Output Format

```
☀️ Good morning! Here's your brief:

**Weather:** 45°F, cloudy, high 52°F

**Today:**
- 10:00 AM: Team standup
- 2:00 PM: 1:1 with Sarah

**Tasks (5):**
1. [H] Set up coding-agent skill
2. [M] Update resume
3. ...

**Repos:** 2 open PRs (mouseion), 0 CI failures

**Infra:** NAS 91%, all containers healthy

**Email:** 3 unread (1 flagged)
```

## Escalation

Only message if:
- Calendar conflict in next 2 hours
- NAS >93%
- Unhealthy containers
- Flagged/urgent email

Otherwise, deliver brief and let Cody start the day.

---

*Template v1 - 2026-01-28*
