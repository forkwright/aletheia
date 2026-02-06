# TOOLS.md - Chiron's Tools

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/aletheia/shared/TOOLS-INFRASTRUCTURE.md) for common commands (gcal, gdrive, tw, letta, pplx, facts, mcporter).

## Code Quality Tools

### Shell Script Linting
```bash
shellcheck script.sh                    # Lint single file
shellcheck -x script.sh                 # Follow sourced files
find . -name "*.sh" -exec shellcheck {} +  # Lint all scripts

# Common issues to check:
# SC2086: Double-quote to prevent globbing
# SC2046: Quote command substitutions
# SC2034: Variable appears unused
```

### Python Linting & Formatting
```bash
ruff check file.py                      # Check for issues
ruff check --fix file.py                # Auto-fix what's possible
ruff format file.py                     # Format code

# Common rules:
# E: pycodestyle errors
# F: pyflakes (undefined names, unused imports)
# I: isort (import sorting)
```

### Code Audit Script
```bash
code-audit                              # Full audit of shared/bin
# Generates report in memory/code-audit-YYYY-MM-DD.md
```

### Useful Patterns
```bash
# Find all Python files
find /mnt/ssd/aletheia -name "*.py" -type f

# Find all shell scripts (by shebang)
grep -rl "^#!/.*sh" /mnt/ssd/aletheia/shared/bin/

# Check for TODO/FIXME comments
grep -rn "TODO\|FIXME\|HACK\|XXX" /path/to/code/
```


## Work Claude Code (Primary Tool)

Control via tmux session on Metis:

```bash
# Check session status
ssh ck@192.168.0.17 'tmux capture-pane -t work-claude -p | tail -20'

# Send a prompt
ssh ck@192.168.0.17 "tmux send-keys -t work-claude 'your prompt here' Enter"

# Wait and read response
sleep 10
ssh ck@192.168.0.17 'tmux capture-pane -t work-claude -p'
```

**Session details:**
- Name: work-claude
- Directory: ~/dianoia/summus
- Config: ~/.claude-work/ (work Anthropic account)
- Permissions: --dangerously-skip-permissions

## Local Reference

Work context synced to: `/mnt/ssd/aletheia/syn/work/`

| Path | Contents |
|------|----------|
| `data_landscape/sql/` | SQL scripts by domain |
| `reporting/dashboards/` | Dashboard projects |
| `gnomon/` | Medical taxonomy |
| `_REGISTRY.md` | Project index |

## SSH to Metis

```bash
ssh ck@192.168.0.17 'command'
```

Metis is Cody's laptop, usually online during work hours.

## Research Tools

**Perplexity (pplx):**
```bash
/mnt/ssd/aletheia/syn/bin/pplx "your query"
/mnt/ssd/aletheia/syn/bin/pplx "your query" --sources  # Include citations
```

**Research wrapper (enhanced):**
```bash
/mnt/ssd/aletheia/syn/bin/research "your query"
/mnt/ssd/aletheia/syn/bin/research "your query" --sources
```

**Web fetch (specific URLs):**
Use the `web_fetch` tool to read specific documentation pages.

**Note:** web_search (Brave) not yet configured. Use pplx/research for now.

## Office Documents (pptx/xlsx)

Extract and manipulate PowerPoint and Excel files:

```bash
# Extract text from PowerPoint
office extract presentation.pptx
office extract presentation.pptx --json    # JSON output

# List slides with titles
office list presentation.pptx

# Extract data from Excel
office extract spreadsheet.xlsx
office extract spreadsheet.xlsx --sheet "Sheet1"  # Specific sheet
office extract spreadsheet.xlsx --json

# List sheets in Excel file
office list spreadsheet.xlsx

# File info
office info document.pptx

# Convert formats (via LibreOffice)
office convert document.pptx --to pdf
office convert spreadsheet.xlsx --to csv
```

**Supported formats:**
- PowerPoint: .pptx, .ppt
- Excel: .xlsx, .xls
- Conversion: pdf, txt, csv, html

**Python libraries available:**
- `openpyxl` - Read/write Excel files
- `python-pptx` - Read/write PowerPoint files
- `libreoffice` - Format conversion

## Letta Memory

Agent: chiron-memory (agent-48ff1c5e-9a65-44bd-9667-33019caa7ef3)

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
letta --agent chiron status
```
