# Upgrading

## `aletheia-memory-mcp` renamed to `xenodocheion`

The standalone stdio MCP memory server — package, crate, and binary — is
renamed from `aletheia-memory-mcp` to `xenodocheion` (GNOMON naming ruling,
aletheia#5584, kanon#540). The old binary name is no longer produced by
`cargo build`/`cargo install`; operators pointing an MCP client config at an
`aletheia-memory-mcp` binary path must update it to `xenodocheion`. The MCP
tool surface (`nous_search`, `nous_neighbors`, `nous_list_topics`,
`nous_stats`, `nous_annotate`, `nous_supersede`, `nous_forget`), CLI flags,
and every `ALETHEIA_MEMORY_MCP_*` environment variable are unchanged.
