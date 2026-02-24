# Credentials signal — check OAuth token file age, warn when approaching expiry
from __future__ import annotations

import os
import time

from loguru import logger

from . import Signal

# Default token lifetime assumptions (seconds)
DEFAULT_WARN_AGE = 6 * 86400  # Warn at 6 days old
DEFAULT_CRITICAL_AGE = 13 * 86400  # Critical at 13 days (tokens typically expire at 14)


async def collect(config: dict) -> list[Signal]:
    cred_config = config.get("signals", {}).get("credentials", {})
    if not cred_config.get("enabled"):
        return []

    warn_age = cred_config.get("warn_age_seconds", DEFAULT_WARN_AGE)
    critical_age = cred_config.get("critical_age_seconds", DEFAULT_CRITICAL_AGE)
    token_files = cred_config.get("token_files", [])
    signals: list[Signal] = []

    for entry in token_files:
        path = os.path.expandvars(os.path.expanduser(entry.get("path", "")))
        label = entry.get("label", os.path.basename(path))

        if not path or not os.path.exists(path):
            signals.append(Signal(
                source="credentials",
                summary=f"Credential file missing: {label}",
                urgency=0.9,
                relevant_nous=["syn"],
                details=f"path={path} status=missing",
            ))
            continue

        try:
            mtime = os.path.getmtime(path)
            age = time.time() - mtime
            age_days = age / 86400

            if age >= critical_age:
                signals.append(Signal(
                    source="credentials",
                    summary=f"CRITICAL: {label} token is {age_days:.0f} days old — likely expired",
                    urgency=0.95,
                    relevant_nous=["syn"],
                    details=f"path={path} age_days={age_days:.1f} threshold={critical_age / 86400:.0f}d",
                ))
            elif age >= warn_age:
                signals.append(Signal(
                    source="credentials",
                    summary=f"Credential aging: {label} is {age_days:.0f} days old",
                    urgency=0.6,
                    relevant_nous=["syn"],
                    details=f"path={path} age_days={age_days:.1f} threshold={warn_age / 86400:.0f}d",
                ))
        except OSError as e:
            logger.warning(f"Failed to stat credential file {path}: {e}")

    return signals
