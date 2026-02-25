# Prior Art Research & Integration Pattern
Systematically discover, clone, and synthesize external systems to identify gaps and learning opportunities for current project design.

## When to Use
When designing a complex system and need to:
- Understand existing solutions in the domain
- Identify design patterns and architectural approaches used by similar systems
- Extract specific workflows, templates, or reference implementations
- Document gaps between your design and proven alternatives
- Build on established best practices rather than reinventing

## Steps
1. Read the current project spec to understand scope and existing design
2. Search for relevant keywords/prior art mentions in your own documentation
3. Enable web search and query for established systems in the domain
4. Clone the most relevant external repository to local filesystem
5. Map the repository structure to understand organization
6. Systematically read key files: README, core workflows, agent descriptions, templates, reference guides
7. Extract reusable patterns (workflows, templates, design principles, checkpoint mechanisms)
8. Synthesize findings into a research document that explicitly calls out what your system should learn
9. Persist findings to project memory with source attribution
10. Create task notes linking findings back to gaps in your current design

## Tools Used
- read: Extract spec context and review external documentation
- exec: Search for prior art mentions, clone repositories, explore directory structures
- enable_tool: Activate web search capability
- web_search: Discover established external systems
- write: Persist research findings to memory
- note: Create actionable tasks linking research to design improvements
