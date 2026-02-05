# Task Namespace Standard

All agents use shared Taskwarrior database with domain-specific namespaces.

## Project Namespaces

| Agent | Project | Subprojects |
|-------|---------|-------------|
| **Chiron** | `project:work` | work.data, work.dashboard, work.gnomon |
| **Eiron** | `project:school` | school.acf, school.strategy, school.capstone |
| **Syl** | `project:home` | home.calendar, home.errands, home.maintenance |
| **Demiurge** | `project:craft` | craft.leather, craft.bindery, craft.joinery |
| **Syn** | `project:infra` | infra.clawdbot, infra.nas, infra.backup |
| **Syn** | `project:personal` | personal.health, personal.finance |

## Standard Tags

| Tag | Meaning |
|-----|---------|
| `+blocked` | Waiting on something |
| `+urgent` | Time-sensitive |
| `+review` | Needs Cody's input |
| `+recurring` | Repeating task |

## Priority Levels

- **H** — Must do today/ASAP
- **M** — This week
- **L** — When convenient

## Commands

```bash
# Add task in namespace
tw add "task description" project:work priority:M +dashboard

# View domain tasks
tw project:work
tw project:school

# Cross-domain view (Syn)
tw project.isnt:          # All tasks
tw +blocked               # All blocked
tw due.before:1w          # Due this week
```

## Agent Responsibilities

Each agent:
1. Manages tasks in their namespace
2. Uses consistent tagging
3. Flags `+blocked` or `+review` when needed
4. Reports task status in status files

Syn:
1. Has visibility across all namespaces
2. Coordinates cross-domain priorities
3. Alerts Cody on conflicts/overload
