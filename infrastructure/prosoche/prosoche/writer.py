# PROSOCHE.md writer — dynamic attention items prepended above static domain checks
from __future__ import annotations

import os
import tempfile
from datetime import datetime, timezone
from pathlib import Path

from loguru import logger

from .scoring import NousScore
from .signals import ContextBlock

DOMAIN_MARKER = "## Domain Checks"
STAGED_MARKER = "## Staged Context"


def update_prosoche(nous_id: str, score: NousScore, nous_root: Path) -> bool:
    prosoche_path = nous_root / nous_id / "PROSOCHE.md"
    if not prosoche_path.parent.exists():
        logger.warning(f"Nous directory missing: {prosoche_path.parent}")
        return False

    static_section = _read_static_section(prosoche_path)
    dynamic_section = _build_dynamic_section(score)
    staged_section = _build_staged_section(score.staged_context)

    if not dynamic_section and not staged_section and not static_section:
        return False

    parts: list[str] = []
    if dynamic_section:
        parts.append(dynamic_section)
    if staged_section:
        parts.append(staged_section)
    if static_section:
        parts.append(static_section)

    content = "\n\n".join(parts) + "\n"

    current = ""
    if prosoche_path.exists():
        current = prosoche_path.read_text()

    if content.strip() == current.strip():
        return False

    fd, tmp_path = tempfile.mkstemp(dir=prosoche_path.parent, suffix=".tmp")
    try:
        os.write(fd, content.encode())
        os.close(fd)
        fd = -1
        os.rename(tmp_path, prosoche_path)
    except BaseException:
        if fd >= 0:
            os.close(fd)
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)
        raise
    logger.info(
        f"Updated PROSOCHE.md for {nous_id} "
        f"({len(score.top_signals)} items, {len(score.staged_context)} staged)"
    )
    return True


def _read_static_section(path: Path) -> str:
    if not path.exists():
        return ""

    text = path.read_text()
    marker_pos = text.find(DOMAIN_MARKER)
    if marker_pos >= 0:
        return text[marker_pos:].strip()
    return ""


def _build_dynamic_section(score: NousScore) -> str:
    if not score.top_signals:
        return ""

    lines = ["## Attention"]
    for signal in score.top_signals:
        if signal.urgency >= 0.8:
            prefix = "[URGENT]"
        elif signal.urgency >= 0.5:
            prefix = "[ATTENTION]"
        else:
            prefix = "[INFO]"
        lines.append(f"- {prefix} {signal.summary}")

    return "\n".join(lines)


def _build_staged_section(blocks: list[ContextBlock]) -> str:
    if not blocks:
        return ""

    now = datetime.now(timezone.utc)
    lines = [STAGED_MARKER, ""]

    for block in blocks:
        lines.append(f"### {block.title}")
        lines.append(f"*Source: {block.source}*")
        if block.expires_at:
            remaining = block.expires_at - now
            mins = int(remaining.total_seconds() / 60)
            if mins > 0:
                lines.append(f"*Expires in ~{mins}min*")
        lines.append("")
        lines.append(block.content)
        lines.append("")

    return "\n".join(lines).rstrip()
