# Browser automation runner — reads task from BROWSE_TASK env var
import asyncio
import os
import sys

from browser_use import Agent
from langchain_anthropic import ChatAnthropic


async def run():
    task = os.environ.get("BROWSE_TASK", "")
    if not task:
        print("Error: BROWSE_TASK environment variable not set", file=sys.stderr)
        sys.exit(1)

    llm = ChatAnthropic(model_name="claude-sonnet-4-20250514")
    agent = Agent(task=task, llm=llm)
    result = await agent.run()
    print(result)


asyncio.run(run())
