# Information Discovery with Fallback Search Strategy
Search for information using multiple strategies when initial methods fail, progressively broadening scope and switching approaches.

## When to Use
When looking for specific information (files, data, or task details) that may exist in various locations or formats, and you need to ensure comprehensive discovery despite initial failures.

## Steps
1. Search for the target in a specific location using precise file patterns
2. Try alternative file patterns in the same location if the first fails
3. Search in a dedicated database or memory system if available
4. Expand the search scope to parent directories with broader path parameters
5. Try alternative file patterns in the expanded scope
6. If file-based searches fail, switch to content-based search (grep) using relevant keywords
7. Read the discovered content to verify and extract the needed information

## Tools Used
- find: Locate files matching specific patterns in defined directories
- mem0_search: Query structured data or memory systems for relevant information
- grep: Search file contents for specific text patterns when file metadata searches fail
- read: Extract and display the full content of located files
