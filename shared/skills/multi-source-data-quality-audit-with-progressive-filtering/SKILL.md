# Multi-Source Data Quality Audit with Progressive Filtering

Systematically evaluate quality across multiple data stores (vector DB, document store, knowledge graph) by sampling, categorizing defects, quantifying noise, and identifying usable vs. garbage data.

## When to Use
When you need to assess the health of a complex multi-backend storage system (vector embeddings, structured documents, knowledge graphs) and identify what portions are actually useful for downstream tasks. Useful for detecting extraction pipeline failures, noisy data ingestion, or graph construction errors.

## Steps
1. Query vector database (Qdrant) with scroll to sample stored memories/facts across all sources
2. Categorize sampled records by quality metrics (length, duplicates, specificity, source)
3. Generate aggregate statistics by agent/source type to identify problematic contributors
4. Query knowledge graph (Neo4j) to examine entity types and relationship distributions
5. Identify garbage entities by finding high-connectivity but semantically meaningless nodes (stopwords, single characters)
6. Quantify noise: calculate percentage of garbage entities and their relationships vs. total
7. Filter to real entities by removing noise patterns and examine their connectivity
8. Test graph traversal on high-quality entities to verify knowledge is actually retrievable
9. Analyze relationship patterns to determine graph structure usefulness (entity-to-entity vs. entity-to-external)
10. Compare actual usage patterns (e.g., grep for query code) against stored data to find disconnects between stored and queried data

## Tools Used
- exec with curl: query vector database scroll endpoints and Neo4j Cypher endpoints for sampling and aggregation
- exec with grep: verify whether storage backends are actually being used by consuming code
