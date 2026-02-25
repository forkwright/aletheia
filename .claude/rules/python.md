# Python Rules

Agent-action rules for the memory sidecar in `infrastructure/memory/sidecar/`.

---

## FastAPI Dependency Injection

Use `fastapi.Depends()` for all dependency injection. Never call dependency functions directly in route parameter defaults.

Compliant:
```python
from fastapi import Depends

def get_db() -> Database:
    return Database(settings.db_url)

@app.post("/search")
async def search(query: SearchQuery, db: Database = Depends(get_db)):
    return await db.search(query.text)
```

Non-compliant:
```python
@app.post("/search")
async def search(query: SearchQuery, db: Database = Database(settings.db_url)):  # B008
    return await db.search(query.text)
```

Use async route handlers whenever the route calls an async service.

See: docs/STANDARDS.md#rule-fastapi-depends-pattern-not-b008

---

## No Bare Exception Catch

Never use bare `except:`. Always specify the exception type. Log exceptions with context before handling.

Compliant:
```python
try:
    result = await mem0.search(query)
except MemorySearchError as e:
    logger.error("Memory search failed", query=query, error=str(e))
    raise HTTPException(status_code=503, detail="Memory search unavailable") from e
```

Non-compliant:
```python
try:
    result = await mem0.search(query)
except:                  # bare except — catches SystemExit, KeyboardInterrupt
    return []

try:
    result = await mem0.search(query)
except Exception:        # swallowed — no log, no raise
    pass
```

`except Exception as e:` is acceptable when the error is logged and handled. `except:` is never acceptable.

See: docs/STANDARDS.md#rule-no-bare-exception-catch

---

## Ruff Rule Set

The sidecar must pass `ruff check` with rules `E`, `W`, `F`, `B`, `I`, `UP` enabled. Do not add `# noqa` suppressions without an inline comment.

Compliant:
```toml
# pyproject.toml
[tool.ruff]
select = ["E", "W", "F", "B", "I", "UP"]
ignore = ["B008"]  # FastAPI Depends() — intentional
```

Non-compliant: no `[tool.ruff]` section in `pyproject.toml`.

Do not add rule sets outside `F, E, W, I, N, B, UP, SIM, TC, RUF` without updating `pyproject.toml`.

See: docs/STANDARDS.md#rule-ruff-selected-rule-set

---

## Type Annotations

Annotate all function parameters and return types. Python 3.12 is available — use modern syntax directly.

Compliant:
```python
async def add_memories(request: AddMemoriesRequest) -> AddMemoriesResponse:
    ...

def get_settings() -> Settings:
    return Settings()
```

Non-compliant:
```python
async def add_memories(request):   # untyped parameter
    ...

def get_settings():                # missing return type
    return Settings()
```

Use `from __future__ import annotations` only when forward references require it — not as a blanket import.

See: docs/STANDARDS.md#rule-pyright-strict-mode
