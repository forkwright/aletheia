# Controlled relationship type vocabulary for Neo4j graph store

CONTROLLED_VOCAB = {
    "KNOWS", "LIVES_IN", "WORKS_AT", "OWNS", "USES", "PREFERS",
    "STUDIES", "MANAGES", "MEMBER_OF", "INTERESTED_IN", "SKILLED_IN",
    "CREATED", "MAINTAINS", "DEPENDS_ON", "LOCATED_IN", "PART_OF",
    "SCHEDULED_FOR", "DIAGNOSED_WITH", "PRESCRIBED", "TREATS",
    "VEHICLE_IS", "INSTALLED_ON", "COMPATIBLE_WITH", "CONNECTED_TO",
    "COMMUNICATES_VIA", "CONFIGURED_WITH", "RUNS_ON", "SERVES",
    "RELATES_TO",
}

TYPE_MAP = {
    "is": "RELATES_TO",
    "has": "OWNS",
    "is_a": "RELATES_TO",
    "is_part_of": "PART_OF",
    "part_of": "PART_OF",
    "works_at": "WORKS_AT",
    "works_on": "WORKS_AT",
    "lives_in": "LIVES_IN",
    "located_in": "LOCATED_IN",
    "located_at": "LOCATED_IN",
    "uses": "USES",
    "used_by": "USES",
    "used_for": "USES",
    "runs_on": "RUNS_ON",
    "runs": "RUNS_ON",
    "depends_on": "DEPENDS_ON",
    "requires": "DEPENDS_ON",
    "knows": "KNOWS",
    "knows_about": "KNOWS",
    "knows_of": "KNOWS",
    "prefers": "PREFERS",
    "likes": "PREFERS",
    "interested_in": "INTERESTED_IN",
    "studies": "STUDIES",
    "studying": "STUDIES",
    "created": "CREATED",
    "created_by": "CREATED",
    "built": "CREATED",
    "made": "CREATED",
    "maintains": "MAINTAINS",
    "managed_by": "MANAGES",
    "manages": "MANAGES",
    "member_of": "MEMBER_OF",
    "belongs_to": "MEMBER_OF",
    "skilled_in": "SKILLED_IN",
    "skilled_at": "SKILLED_IN",
    "owns": "OWNS",
    "has_a": "OWNS",
    "installed_on": "INSTALLED_ON",
    "installed": "INSTALLED_ON",
    "compatible_with": "COMPATIBLE_WITH",
    "connected_to": "CONNECTED_TO",
    "communicates_via": "COMMUNICATES_VIA",
    "configured_with": "CONFIGURED_WITH",
    "serves": "SERVES",
    "diagnosed_with": "DIAGNOSED_WITH",
    "prescribed": "PRESCRIBED",
    "treats": "TREATS",
    "scheduled_for": "SCHEDULED_FOR",
    "vehicle_is": "VEHICLE_IS",
    "relates_to": "RELATES_TO",
}

KEYWORD_MAP = {
    "know": "KNOWS",
    "live": "LIVES_IN",
    "work": "WORKS_AT",
    "own": "OWNS",
    "use": "USES",
    "prefer": "PREFERS",
    "stud": "STUDIES",
    "manag": "MANAGES",
    "member": "MEMBER_OF",
    "interest": "INTERESTED_IN",
    "skill": "SKILLED_IN",
    "creat": "CREATED",
    "maintain": "MAINTAINS",
    "depend": "DEPENDS_ON",
    "locat": "LOCATED_IN",
    "part": "PART_OF",
    "schedul": "SCHEDULED_FOR",
    "diagnos": "DIAGNOSED_WITH",
    "prescri": "PRESCRIBED",
    "treat": "TREATS",
    "vehicle": "VEHICLE_IS",
    "install": "INSTALLED_ON",
    "compat": "COMPATIBLE_WITH",
    "connect": "CONNECTED_TO",
    "communic": "COMMUNICATES_VIA",
    "config": "CONFIGURED_WITH",
    "run": "RUNS_ON",
    "serv": "SERVES",
}


def normalize_type(rel_type: str) -> str:
    """Map a relationship type to controlled vocabulary."""
    if rel_type in CONTROLLED_VOCAB:
        return rel_type

    lower = rel_type.lower().strip()
    if lower in TYPE_MAP:
        return TYPE_MAP[lower]

    normalized = lower.replace(" ", "_").replace("-", "_")
    if normalized in TYPE_MAP:
        return TYPE_MAP[normalized]

    for keyword, target in KEYWORD_MAP.items():
        if keyword in normalized:
            return target

    return "RELATES_TO"
