# STYLE.md - Coding & Documentation Standards

These preferences apply to all code I create or review.

## Shell Scripts (Bash)

- Use `#!/bin/bash` shebang (not `#!/bin/sh` unless POSIX required)
- Quote variables: `"$VAR"` not `$VAR`
- Use `[[ ]]` for conditionals (not `[ ]`)
- Prefer `$(command)` over backticks
- Use `set -euo pipefail` for strict mode in critical scripts
- Add usage/help for any script with args
- Comments for non-obvious logic

```bash
# Good
if [[ -n "$VAR" ]]; then
    result=$(some_command "$VAR")
fi

# Bad
if [ -n $VAR ]; then
    result=`some_command $VAR`
fi
```

## Python

- Python 3.10+ features OK (match, walrus, union types)
- Type hints for function signatures
- Use `pathlib` over `os.path`
- Prefer f-strings over `.format()`
- Use `typer` for CLIs, `loguru` for logging
- Use `uv` for package management
- Keep scripts executable with `if __name__ == "__main__":`

```python
# Good
from pathlib import Path

def process_file(path: Path, verbose: bool = False) -> dict:
    """Process a file and return results."""
    ...

# Bad
import os
def process_file(path, verbose=False):
    ...
```

## Markdown/Documentation

- ATX headers (`#` not underlines)
- Fenced code blocks with language tags
- Tables for structured data
- Keep lines under 120 chars when reasonable
- Use relative links for local files
- Front matter for metadata when applicable

## File Organization

- Scripts in `bin/` with no extension
- Python tools can have `.py` extension
- Config files: YAML > JSON > TOML
- One purpose per file

## Naming

- Scripts: `kebab-case` (e.g., `self-audit`, `health-watchdog`)
- Python: `snake_case` for files and functions
- Directories: `lowercase` or `kebab-case`
- Constants: `UPPER_SNAKE_CASE`

## Error Handling

- Fail fast, fail loud
- Provide actionable error messages
- Exit codes: 0 = success, 1 = error, 2 = usage error
- Log errors to stderr

## Security

- No secrets in code (use env vars or config)
- Validate/sanitize inputs
- Use `trash` over `rm` for recoverable deletes
- Principle of least privilege

## Review Checklist

Before committing:
- [ ] shellcheck passes (for bash)
- [ ] ruff check passes (for python)
- [ ] No hardcoded secrets
- [ ] Help/usage documented
- [ ] Error cases handled

---

*Updated: 2026-01-29*
