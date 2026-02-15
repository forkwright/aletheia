#!/usr/bin/env python3
# Nightly eval feedback — generates per-agent EVAL_FEEDBACK.md from competence + calibration data

import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

from loguru import logger

logger.remove()
logger.add(sys.stderr, format="{time:HH:mm:ss} | {level:<7} | {message}", level="INFO")

ALETHEIA_HOME = Path(os.environ.get("ALETHEIA_HOME", "/mnt/ssd/aletheia"))
NOUS_DIR = ALETHEIA_HOME / "nous"
SHARED_DIR = ALETHEIA_HOME / "shared"
COMPETENCE_FILE = SHARED_DIR / "competence" / "model.json"
CALIBRATION_FILE = SHARED_DIR / "calibration" / "points.json"
CONSOLIDATION_LOG = SHARED_DIR / "memory" / "consolidation-log.jsonl"
EVAL_RESULTS = SHARED_DIR / "evaluation" / "adversarial-results.json"


def load_competence() -> dict:
    if not COMPETENCE_FILE.exists():
        return {}
    try:
        return json.loads(COMPETENCE_FILE.read_text())
    except Exception as e:
        logger.warning(f"Failed to load competence: {e}")
        return {}


def load_calibration() -> list[dict]:
    if not CALIBRATION_FILE.exists():
        return []
    try:
        return json.loads(CALIBRATION_FILE.read_text())
    except Exception as e:
        logger.warning(f"Failed to load calibration: {e}")
        return []


def compute_agent_calibration(points: list[dict], nous_id: str) -> dict:
    relevant = [p for p in points if p.get("nousId") == nous_id]
    if not relevant:
        return {"points": 0}

    correct = sum(1 for p in relevant if p.get("wasCorrect"))
    total = len(relevant)

    # Brier score
    brier = sum(
        (p.get("statedConfidence", 0.5) - (1 if p.get("wasCorrect") else 0)) ** 2
        for p in relevant
    ) / total

    # Recent trend (last 20 vs overall)
    recent = relevant[-20:]
    recent_correct = sum(1 for p in recent if p.get("wasCorrect"))
    recent_accuracy = recent_correct / len(recent) if recent else 0

    return {
        "points": total,
        "accuracy": round(correct / total, 3),
        "brier_score": round(brier, 3),
        "recent_accuracy": round(recent_accuracy, 3),
        "trending": "improving" if recent_accuracy > (correct / total + 0.05) else
                    "declining" if recent_accuracy < (correct / total - 0.05) else "stable",
    }


def load_adversarial_summary() -> dict | None:
    if not EVAL_RESULTS.exists():
        return None
    try:
        data = json.loads(EVAL_RESULTS.read_text())
        return data.get("summary")
    except Exception:
        return None


def generate_feedback(nous_id: str, competence: dict, calibration: dict, adversarial: dict | None) -> str:
    lines = [
        f"# Eval Feedback — {nous_id}",
        f"*Generated {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}*",
        "",
    ]

    # Competence summary
    agent = competence.get(nous_id, {})
    domains = agent.get("domains", {})
    overall = agent.get("overallScore", 0.5)

    lines.append("## Competence")
    lines.append(f"Overall: {overall:.2f}")
    lines.append("")

    if domains:
        strengths = sorted(
            [(k, v) for k, v in domains.items() if v.get("score", 0.5) >= 0.6],
            key=lambda x: x[1].get("score", 0),
            reverse=True,
        )
        weaknesses = sorted(
            [(k, v) for k, v in domains.items() if v.get("score", 0.5) < 0.4 or v.get("corrections", 0) >= 3],
            key=lambda x: x[1].get("score", 1),
        )

        if strengths:
            lines.append("**Strengths:**")
            for name, d in strengths[:5]:
                lines.append(f"- {name}: {d['score']:.2f} ({d.get('successes', 0)} verified)")
            lines.append("")

        if weaknesses:
            lines.append("**Areas for caution:**")
            for name, d in weaknesses[:5]:
                lines.append(f"- {name}: {d['score']:.2f} ({d.get('corrections', 0)} corrections)")
            lines.append("")

        # Correction patterns
        high_corrections = [(k, v) for k, v in domains.items() if v.get("corrections", 0) >= 2]
        if high_corrections:
            lines.append("**Correction patterns:**")
            for name, d in sorted(high_corrections, key=lambda x: x[1].get("corrections", 0), reverse=True)[:3]:
                lines.append(f"- {name}: {d['corrections']} corrections — slow down and verify in this area")
            lines.append("")
    else:
        lines.append("No domain data recorded yet.")
        lines.append("")

    # Calibration
    lines.append("## Calibration")
    if calibration.get("points", 0) > 0:
        lines.append(f"Data points: {calibration['points']}")
        lines.append(f"Accuracy: {calibration.get('accuracy', 0):.1%}")
        lines.append(f"Brier score: {calibration.get('brier_score', 0.5):.3f} (lower is better)")
        lines.append(f"Recent trend: {calibration.get('trending', 'unknown')}")
        lines.append("")

        brier = calibration.get("brier_score", 0.5)
        if brier > 0.3:
            lines.append("Your confidence estimates are poorly calibrated. Express less certainty on borderline answers.")
        elif brier < 0.15:
            lines.append("Your confidence estimates are well-calibrated. Keep it up.")
        lines.append("")
    else:
        lines.append("No calibration data yet.")
        lines.append("")

    # Adversarial
    if adversarial:
        lines.append("## Adversarial Tests")
        lines.append(f"Last run: {adversarial.get('injection_count', 0)} injection, {adversarial.get('sycophancy_count', 0)} sycophancy")
        lines.append("")

    return "\n".join(lines)


def main() -> None:
    dry_run = "--dry-run" in sys.argv

    competence = load_competence()
    calibration_points = load_calibration()
    adversarial = load_adversarial_summary()

    # Find all agent workspaces
    if not NOUS_DIR.exists():
        logger.error(f"Nous directory not found: {NOUS_DIR}")
        sys.exit(1)

    agents = [d.name for d in NOUS_DIR.iterdir() if d.is_dir()]
    logger.info(f"Found {len(agents)} agent workspaces: {', '.join(agents)}")

    for agent in agents:
        cal = compute_agent_calibration(calibration_points, agent)
        feedback = generate_feedback(agent, competence, cal, adversarial)

        out_path = NOUS_DIR / agent / "EVAL_FEEDBACK.md"
        if dry_run:
            print(f"\n{'='*40} {agent} {'='*40}")
            print(feedback)
        else:
            out_path.write_text(feedback)
            logger.info(f"Wrote {out_path} ({len(feedback)} bytes)")

    logger.info("Eval feedback generation complete")


if __name__ == "__main__":
    main()
