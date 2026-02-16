# Per-nous attention scoring
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone

from .signals import ContextBlock, Signal, SignalBundle


@dataclass
class NousScore:
    nous_id: str
    score: float
    top_signals: list[Signal] = field(default_factory=list)
    staged_context: list[ContextBlock] = field(default_factory=list)
    should_wake: bool = False


def score_nous(
    nous_id: str,
    bundle: SignalBundle,
    weights: dict[str, float],
    urgent_threshold: float = 0.8,
) -> NousScore:
    relevant = bundle.for_nous(nous_id)
    if not relevant:
        return NousScore(nous_id=nous_id, score=0.0)

    weighted_scores = []
    for signal in relevant:
        weight = weights.get(signal.source, 0.1)
        weighted_scores.append((signal, signal.urgency * weight))

    weighted_scores.sort(key=lambda x: x[1], reverse=True)

    if not weighted_scores:
        return NousScore(nous_id=nous_id, score=0.0)

    top_score = weighted_scores[0][1]
    avg_score = sum(s for _, s in weighted_scores) / len(weighted_scores)
    composite = top_score * 0.7 + avg_score * 0.3

    top_signals = [s for s, _ in weighted_scores[:5]]

    # Collect staged context from all relevant signals, drop expired blocks
    now = datetime.now(timezone.utc)
    staged: list[ContextBlock] = []
    for signal in relevant:
        for block in signal.context_blocks:
            if block.expires_at is None or block.expires_at > now:
                staged.append(block)

    return NousScore(
        nous_id=nous_id,
        score=composite,
        top_signals=top_signals,
        staged_context=staged,
        should_wake=any(s.urgency >= urgent_threshold for s in top_signals),
    )
