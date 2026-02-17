#!/usr/bin/env python3
# Browser automation runner — LLM-driven multi-step browsing via browser-use
import asyncio
import json
import os
import signal
import sys

from browser_use import Agent
from langchain_anthropic import ChatAnthropic

TIMEOUT = int(os.environ.get("BROWSE_TIMEOUT", "120"))


async def run():
    task = os.environ.get("BROWSE_TASK", "")
    if not task:
        print(json.dumps({"error": "BROWSE_TASK environment variable not set"}))
        sys.exit(1)

    model = os.environ.get("BROWSE_MODEL", "claude-sonnet-4-20250514")
    llm = ChatAnthropic(model_name=model)
    agent = Agent(task=task, llm=llm)

    try:
        result = await asyncio.wait_for(agent.run(), timeout=TIMEOUT)
        print(json.dumps({
            "success": True,
            "result": str(result)[:10000],
            "task": task,
        }))
    except asyncio.TimeoutError:
        print(json.dumps({
            "success": False,
            "error": f"Task timed out after {TIMEOUT}s",
            "task": task,
        }))
        sys.exit(1)
    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e)[:2000],
            "task": task,
        }))
        sys.exit(1)


def timeout_handler(_signum, _frame):
    print(json.dumps({"success": False, "error": "Process killed by timeout signal"}))
    sys.exit(1)


signal.signal(signal.SIGALRM, timeout_handler)
signal.alarm(TIMEOUT + 10)
asyncio.run(run())
