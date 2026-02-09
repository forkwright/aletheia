#!/usr/bin/env python3
# QA audit: sample memories per agent, flag low-quality extractions

import json
import random
import sys
from collections import defaultdict

import httpx

SIDECAR_URL = "http://127.0.0.1:8230"
QDRANT_URL = "http://localhost:6333"
COLLECTION = "aletheia_memories"
SAMPLE_SIZE = 20

JUNK_INDICATORS = [
    "error", "traceback", "stacktrace", "errno", "eacces", "enoent",
    "failed to", "exception", "status_code", "http/1.1",
    "import ", "from ", "def ", "class ", "async def",
    "```", "#!/", "function(", "const ", "let ", "var ",
    "node_modules", ".venv", "__pycache__",
    "tool_use", "tool_result", "content_block",
]

NOISE_PATTERNS = [
    lambda t: len(t) < 20,
    lambda t: t.count("/") > 5,
    lambda t: t.count("=") > 10,
    lambda t: any(t.startswith(p) for p in ["http://", "https://", "ssh ", "scp "]),
]


def get_all_points():
    points = []
    offset = None
    while True:
        body = {"limit": 100, "with_payload": True, "with_vector": False}
        if offset:
            body["offset"] = offset
        resp = httpx.post(
            f"{QDRANT_URL}/collections/{COLLECTION}/points/scroll",
            json=body,
            timeout=30.0,
        )
        resp.raise_for_status()
        data = resp.json()["result"]
        batch = data.get("points", [])
        if not batch:
            break
        points.extend(batch)
        offset = data.get("next_page_offset")
        if not offset:
            break
    return points


def classify_quality(memory_text: str) -> tuple[str, list[str]]:
    text_lower = memory_text.lower()
    issues = []

    for indicator in JUNK_INDICATORS:
        if indicator in text_lower:
            issues.append(f"contains '{indicator}'")

    for i, check in enumerate(NOISE_PATTERNS):
        try:
            if check(memory_text):
                labels = ["too short", "path-heavy", "equals-heavy", "raw URL"]
                issues.append(labels[i])
        except Exception:
            pass

    if len(issues) >= 3:
        return "junk", issues
    elif len(issues) >= 1:
        return "suspect", issues
    return "clean", []


def main():
    print("Fetching all memories from Qdrant...", flush=True)
    points = get_all_points()
    print(f"Total memories: {len(points)}\n", flush=True)

    by_agent = defaultdict(list)
    no_agent = []
    for p in points:
        payload = p.get("payload", {})
        agent = payload.get("agent_id") or payload.get("metadata", {}).get("agent") or "shared"
        by_agent[agent].append(p)

    stats = {"clean": 0, "suspect": 0, "junk": 0}
    junk_ids = []
    suspect_ids = []

    for agent in sorted(by_agent.keys()):
        agent_points = by_agent[agent]
        sample = random.sample(agent_points, min(SAMPLE_SIZE, len(agent_points)))

        agent_stats = {"clean": 0, "suspect": 0, "junk": 0}
        agent_issues = []

        for p in sample:
            payload = p.get("payload", {})
            memory = payload.get("memory", payload.get("data", ""))
            if not memory:
                memory = str(payload)

            quality, issues = classify_quality(memory)
            agent_stats[quality] += 1
            stats[quality] += 1

            if quality == "junk":
                junk_ids.append(p["id"])
                agent_issues.append(("JUNK", memory[:120], issues))
            elif quality == "suspect":
                suspect_ids.append(p["id"])
                agent_issues.append(("SUSPECT", memory[:120], issues))

        total_sampled = sum(agent_stats.values())
        junk_pct = agent_stats["junk"] / total_sampled * 100 if total_sampled else 0
        print(f"=== {agent} ({len(agent_points)} total, {total_sampled} sampled) ===")
        print(f"  Clean: {agent_stats['clean']}  Suspect: {agent_stats['suspect']}  Junk: {agent_stats['junk']} ({junk_pct:.0f}%)")

        if agent_issues:
            for label, text, issues in agent_issues[:5]:
                print(f"  [{label}] {text}...")
                print(f"    Reasons: {', '.join(issues)}")
        print()

    total_sampled = sum(stats.values())
    print("=" * 60)
    print(f"OVERALL ({total_sampled} sampled across {len(by_agent)} agents):")
    print(f"  Clean: {stats['clean']}  Suspect: {stats['suspect']}  Junk: {stats['junk']}")
    if total_sampled:
        print(f"  Junk rate: {stats['junk'] / total_sampled * 100:.1f}%")
        print(f"  Suspect rate: {stats['suspect'] / total_sampled * 100:.1f}%")

    if junk_ids:
        print(f"\n{len(junk_ids)} junk IDs found in sample (extrapolated: ~{len(junk_ids) / total_sampled * len(points):.0f} total)")
        print("Run with --delete to remove all junk memories")

    if "--delete" in sys.argv:
        print(f"\nDeleting {len(junk_ids)} junk memories...")
        client = httpx.Client(timeout=10.0)
        deleted = 0
        for mid in junk_ids:
            try:
                resp = client.delete(f"{SIDECAR_URL}/memories/{mid}")
                if resp.status_code == 200:
                    deleted += 1
            except Exception as e:
                print(f"  Failed to delete {mid}: {e}")
        client.close()
        print(f"Deleted {deleted}/{len(junk_ids)}")

    if "--delete-suspect" in sys.argv:
        print(f"\nDeleting {len(suspect_ids)} suspect memories...")
        client = httpx.Client(timeout=10.0)
        deleted = 0
        for mid in suspect_ids:
            try:
                resp = client.delete(f"{SIDECAR_URL}/memories/{mid}")
                if resp.status_code == 200:
                    deleted += 1
            except Exception as e:
                print(f"  Failed to delete {mid}: {e}")
        client.close()
        print(f"Deleted {deleted}/{len(suspect_ids)}")


if __name__ == "__main__":
    main()
