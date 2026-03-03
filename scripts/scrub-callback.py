# commit message scrubbing callback for git-filter-repo --message-callback
import re

# Remove Co-authored-by trailers (all 5 case variants found in history)
message = re.sub(rb'(?im)^co-authored-by:.*$\n?', b'', message)

# Remove internal IPs (192.168.x.x and 100.x.x.x Tailscale ranges)
message = re.sub(rb'\b192\.168\.\d{1,3}\.\d{1,3}\b', b'[internal]', message)
message = re.sub(rb'\b100\.\d{1,3}\.\d{1,3}\.\d{1,3}\b', b'[internal]', message)

# Remove work email from message bodies (author fields handled by --mailmap)
message = re.sub(rb'\bforkwright@acme-corp\.com\b', b'[redacted]', message)

# Remove noreply AI attribution (belt-and-suspenders — also caught by Co-authored-by removal)
message = re.sub(rb'(?im)^.*noreply@anthropic\.com.*$\n?', b'', message)

# Remove secrets — token=..., password=..., api_key=..., bearer tokens
message = re.sub(rb'(?i)((?:token|password|api_key|secret|bearer)\s*[=:]\s*)\S+', rb'\1[redacted]', message)

# Remove common token prefixes (sk-..., ghp_..., gho_...)
message = re.sub(rb'(?:sk-|ghp_|gho_)[A-Za-z0-9_-]+', b'[redacted]', message)

# Location details
message = re.sub(rb'(?i)\bSpringfield\b', b'[location]', message)

# Personal email in message bodies
message = re.sub(rb'\bcody\.johnson@gmail\.com\b', b'[redacted]', message)

# Normalize trailing blank lines (Pitfall 7: removals leave orphaned blank lines)
message = re.sub(rb'\n{3,}', b'\n\n', message)
message = message.rstrip() + b'\n'
