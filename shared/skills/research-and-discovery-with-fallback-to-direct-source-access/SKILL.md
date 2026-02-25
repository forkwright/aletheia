# Research and Discovery with Fallback to Direct Source Access
Systematically search for information on a topic using multiple query variations, then fetch directly from identified sources when search results are insufficient.

## When to Use
When you need to research a specific tool, library, or concept that may have limited web presence, and you've identified a likely source (like a GitHub repository) but need to verify its contents and gather detailed information.

## Steps
1. Enable web_search tool to begin research
2. Execute initial web search with primary keywords and filters (license type, date range, platform)
3. Analyze results and refine search strategy based on findings
4. Execute follow-up web searches with alternative keyword combinations targeting specific aspects (provider, framework, language, features)
5. If searches yield insufficient results, enable web_fetch tool
6. Fetch content directly from the most promising identified source (e.g., GitHub repository)
7. Persist findings and categorize the discovered information for project tracking

## Tools Used
- web_search: Find relevant information across the web using iterative query refinement
- web_fetch: Extract detailed content directly from identified sources when search results are incomplete
- plan_requirements: Organize and persist discovered information within a project structure
