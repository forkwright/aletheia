"""LLM-based entity and relationship extraction."""

import json
import requests
from typing import NamedTuple

OLLAMA_URL = "http://localhost:11434"
EXTRACT_MODEL = "qwen2.5:3b"  # Fast, good at structured output

EXTRACT_PROMPT = """Extract entities and relationships from this text.

Text: {text}

Return JSON with this structure:
{{
  "entities": [
    {{"name": "...", "type": "person|place|thing|concept|event|project", "description": "..."}}
  ],
  "relationships": [
    {{"from": "entity1", "to": "entity2", "type": "USES|DECIDED|LEARNED|CREATED|MENTIONS|WORKS_ON|BELONGS_TO", "description": "..."}}
  ]
}}

Only include entities and relationships that are clearly stated or strongly implied.
Return valid JSON only, no explanation."""


class Entity(NamedTuple):
    name: str
    type: str
    description: str


class Relationship(NamedTuple):
    from_entity: str
    to_entity: str
    rel_type: str
    description: str


def extract(text: str) -> tuple[list[Entity], list[Relationship]]:
    """Extract entities and relationships using LLM."""
    
    prompt = EXTRACT_PROMPT.format(text=text)
    
    try:
        resp = requests.post(
            f"{OLLAMA_URL}/api/generate",
            json={
                "model": EXTRACT_MODEL,
                "prompt": prompt,
                "stream": False,
                "format": "json"
            },
            timeout=30
        )
        resp.raise_for_status()
        
        result = resp.json()
        output = result.get("response", "{}")
        
        # Parse JSON
        data = json.loads(output)
        
        entities = [
            Entity(
                name=e.get("name", ""),
                type=e.get("type", "thing"),
                description=e.get("description", "")
            )
            for e in data.get("entities", [])
            if e.get("name")
        ]
        
        relationships = [
            Relationship(
                from_entity=r.get("from", ""),
                to_entity=r.get("to", ""),
                rel_type=r.get("type", "MENTIONS"),
                description=r.get("description", "")
            )
            for r in data.get("relationships", [])
            if r.get("from") and r.get("to")
        ]
        
        return entities, relationships
        
    except Exception as e:
        print(f"LLM extraction failed: {e}")
        return [], []


if __name__ == "__main__":
    # Test
    test_text = """Cody started a new wallet project using Hermann Oak leather. 
    He decided to use Fil au Chinois thread for durability. 
    The edge will be dyed with Aima once stitching is complete."""
    
    entities, rels = extract(test_text)
    print("Entities:")
    for e in entities:
        print(f"  {e.name} ({e.type}): {e.description}")
    print("\nRelationships:")
    for r in rels:
        print(f"  {r.from_entity} -{r.rel_type}-> {r.to_entity}: {r.description}")
