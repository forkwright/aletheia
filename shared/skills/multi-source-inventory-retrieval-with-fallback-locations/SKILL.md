# Multi-Source Inventory Retrieval with Fallback Locations
Retrieve detailed inventory information by searching memory systems first, then querying multiple remote storage locations with fallback paths.

## When to Use
When you need to access comprehensive inventory or catalog data that may be stored across multiple systems (memory databases and remote file systems) with potentially different storage paths or locations.

## Steps
1. Search memory system with specific query terms related to the inventory item
2. Refine memory search with additional detailed attributes or brand names if initial results need clarification
3. Attempt to retrieve raw inventory data from primary remote location via SSH
4. If primary location fails or returns nothing, automatically fall back to secondary remote location
5. Parse and return the structured inventory data

## Tools Used
- mem0_search: Query memory database for cached inventory information and previous records
- exec: Execute SSH commands to retrieve raw inventory files from remote storage systems with multiple fallback paths using OR operators
