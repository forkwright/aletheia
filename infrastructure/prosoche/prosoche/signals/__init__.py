# Signal collectors for prosoche
from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Signal:
    source: str
    summary: str
    urgency: float  # 0.0 = informational, 1.0 = critical
    relevant_nous: list[str] = field(default_factory=list)
    details: str = ""


@dataclass
class SignalBundle:
    signals: list[Signal] = field(default_factory=list)
    collected_at: float = 0.0

    def for_nous(self, nous_id: str) -> list[Signal]:
        return [s for s in self.signals if not s.relevant_nous or nous_id in s.relevant_nous]
