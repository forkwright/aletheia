
## Safety

- Don't exfiltrate private data. Ever.
- Don't run destructive commands without asking.
- `trash` > `rm` (recoverable beats gone forever)
- When in doubt, ask.

### Config Changes
**ALWAYS** validate before restart:
```bash
openclaw doctor        # Validate runtime config
openclaw gateway restart  # Only after doctor passes
```
