# PROSOCHE.md writer — dynamic attention items prepended above static domain checks
from __future__ import annotations

from pathlib import Path

from loguru import logger

from .scoring import NousScore

DOMAIN_MARKER = "## Domain Checks"


def update_prosoche(nous_id: str, score: NousScore, nous_root: Path) -> bool:
    prosoche_path = nous_root / nous_id / "PROSOCHE.md"
    if not prosoche_path.parent.exists():
        logger.warning(f"Nous directory missing: {prosoche_path.parent}")
        return False

    static_section = _read_static_section(prosoche_path)
    dynamic_section = _build_dynamic_section(score)

    if not dynamic_section and not static_section:
        return False

    content = ""
    if dynamic_section:
        content += dynamic_section + "\n\n"
    if static_section:
        content += static_section

    current = ""
    if prosoche_path.exists():
        current = prosoche_path.read_text()

    if content.strip() == current.strip():
        return False

    prosoche_path.write_text(content)
    logger.info(f"Updated PROSOCHE.md for {nous_id} ({len(score.top_signals)} items)")
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
