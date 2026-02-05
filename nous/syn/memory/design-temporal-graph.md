# Event Graph / Temporal Reasoning System Design

**Version:** 1.0  
**Date:** 2026-02-03  
**Author:** Temporal Graph Research Agent

## Executive Summary

This document outlines the design for a comprehensive temporal reasoning system that extends our existing graph database architecture. The system will store events with temporal relationships, support complex temporal queries, and integrate seamlessly with our current facts.jsonl and memory infrastructure.

## Research Foundation

### Temporal Graph Modeling Patterns

From research on temporal graph databases and Allen's Interval Algebra, the key principles are:

1. **Event-Centric Modeling:** Events are first-class entities with temporal properties
2. **Interval-Based Reasoning:** Use Allen's 13 temporal relations (before, meets, overlaps, etc.)
3. **Temporal Edges:** Relationships that encode temporal semantics directly in the graph structure
4. **Hybrid Storage:** Time as both properties and structural elements for efficient querying

### Allen's Interval Algebra Foundation

The system is built on Allen's 13 base temporal relations:
- **before/after** (precedes/preceded-by)
- **meets/met-by** (adjacent intervals)
- **overlaps/overlapped-by** (partial overlap)
- **starts/started-by** (same start, different end)
- **during/contains** (one interval within another)
- **finishes/finished-by** (same end, different start)
- **equals** (identical intervals)

## Data Model

### Node Types

#### 1. Event Nodes
```cypher
(:Event {
  id: string,                    // Unique event identifier
  name: string,                  // Human-readable event name
  description: string,           // Detailed description
  start_time: datetime,          // Event start (ISO 8601)
  end_time: datetime,            // Event end (can be null for instantaneous)
  duration: duration,            // Computed duration
  event_type: string,            // Category (action, state_change, milestone, etc.)
  confidence: float,             // 0.0-1.0 confidence score
  source: string,                // Where this event was recorded
  extracted_at: datetime,        // When it was added to the system
  tags: [string],                // Searchable tags
  embedding: [float]             // Semantic embedding vector (768d)
})
```

#### 2. Fact Nodes (Enhanced from facts.jsonl)
```cypher
(:Fact {
  id: string,                    // Original fact ID
  subject: string,               // Fact subject
  predicate: string,             // Fact predicate
  object: string,                // Fact object
  confidence: float,             // Confidence score
  category: string,              // Fact category
  valid_from: datetime,          // Temporal validity start
  valid_to: datetime,            // Temporal validity end (nullable)
  embedding: [float]             // Semantic embedding
})
```

#### 3. Time Interval Nodes
```cypher
(:Interval {
  id: string,                    // Unique interval ID
  start: datetime,               // Interval start
  end: datetime,                 // Interval end (nullable for ongoing)
  duration: duration,            // Computed duration
  label: string,                 // Human description ("Q4 2025", "morning routine")
  granularity: string            // second, minute, hour, day, week, month, year
})
```

#### 4. Entity Nodes (Existing from current graph)
```cypher
(:Entity {
  name: string,
  type: string,
  embedding: [float]
})
```

### Edge Types

#### 1. Temporal Relationships (Allen's Relations)
```cypher
// Direct temporal relationships between events
(:Event)-[:BEFORE]->(:Event)          // X precedes Y
(:Event)-[:MEETS]->(:Event)           // X meets Y (adjacent)
(:Event)-[:OVERLAPS]->(:Event)        // X overlaps with Y
(:Event)-[:STARTS]->(:Event)          // X starts Y
(:Event)-[:DURING]->(:Event)          // X during Y
(:Event)-[:FINISHES]->(:Event)        // X finishes Y
(:Event)-[:EQUALS]->(:Event)          // X equals Y

// Inverse relationships (automatically inferred)
(:Event)-[:AFTER]->(:Event)           // Y after X
(:Event)-[:MET_BY]->(:Event)          // Y met by X
// etc.
```

#### 2. Causal Relationships
```cypher
(:Event)-[:CAUSED_BY {
  confidence: float,
  delay: duration,                     // Time between cause and effect
  evidence: string
}]->(:Event)

(:Event)-[:TRIGGERED {
  confidence: float
}]->(:Event)
```

#### 3. Containment Relationships
```cypher
(:Event)-[:OCCURS_DURING]->(:Interval)
(:Fact)-[:VALID_DURING]->(:Interval)
(:Event)-[:PART_OF]->(:Event)         // Sub-events
```

#### 4. Entity-Event Relationships
```cypher
(:Entity)-[:PARTICIPATES_IN {
  role: string,                        // "agent", "patient", "location", etc.
  involvement_level: float
}]->(:Event)

(:Event)-[:AFFECTS {
  change_type: string,                 // "created", "modified", "deleted"
  old_value: string,
  new_value: string
}]->(:Entity)
```

#### 5. Sequential Relationships
```cypher
(:Event)-[:NEXT {
  sequence_id: string,                 // Which sequence this belongs to
  position: integer                    // Position in sequence
}]->(:Event)

(:Event)-[:FIRST_IN_SEQUENCE]->(:Event)
(:Event)-[:LAST_IN_SEQUENCE]->(:Event)
```

## Query Patterns

### 1. Basic Temporal Queries

#### What happened before/after event X?
```cypher
// Events before X
MATCH (x:Event {name: "project launch"})<-[:BEFORE]-(before:Event)
RETURN before.name, before.start_time
ORDER BY before.start_time DESC

// Events after X  
MATCH (x:Event {name: "project launch"})-[:BEFORE]->(after:Event)
RETURN after.name, after.start_time
ORDER BY after.start_time ASC
```

#### Events during a time period
```cypher
MATCH (e:Event)
WHERE e.start_time >= datetime('2026-01-01T00:00:00Z')
  AND e.end_time <= datetime('2026-01-31T23:59:59Z')
RETURN e.name, e.start_time, e.end_time
ORDER BY e.start_time
```

#### Overlapping events
```cypher
MATCH (e1:Event)-[:OVERLAPS]->(e2:Event)
RETURN e1.name as event1, e2.name as event2, 
       e1.start_time, e1.end_time,
       e2.start_time, e2.end_time
```

### 2. Sequence Reasoning

#### Find event sequences
```cypher
MATCH path = (start:Event)-[:NEXT*]->(end:Event)
WHERE NOT (start)<-[:NEXT]-()  // start has no predecessor
  AND NOT (end)-[:NEXT]->()    // end has no successor
RETURN [n in nodes(path) | n.name] as sequence,
       start.start_time as sequence_start,
       end.end_time as sequence_end
```

#### What typically happens after X?
```cypher
MATCH (x:Event {name: "morning coffee"})-[:BEFORE]->(after:Event)
RETURN after.name, count(*) as frequency
ORDER BY frequency DESC
LIMIT 10
```

### 3. Causal Reasoning

#### Find causal chains
```cypher
MATCH path = (root:Event)-[:CAUSED_BY*1..5]->(effect:Event)
RETURN [n in nodes(path) | n.name] as causal_chain,
       length(path) as chain_length
ORDER BY chain_length DESC
```

#### What led to outcome Y?
```cypher
MATCH (outcome:Event {name: "system crash"})<-[:CAUSED_BY*]-(cause:Event)
RETURN cause.name, cause.start_time,
       shortestPath((cause)-[:CAUSED_BY*]->(outcome)) as causal_path
```

### 4. Complex Temporal Reasoning

#### Events that happened during overlapping intervals
```cypher
MATCH (i1:Interval)<-[:OCCURS_DURING]-(e1:Event)
MATCH (i2:Interval)<-[:OCCURS_DURING]-(e2:Event)
MATCH (i1)-[:OVERLAPS]-(i2)
RETURN e1.name, e2.name, i1.label, i2.label
```

#### Find temporal patterns
```cypher
// Events that always happen in a specific order
MATCH (a:Event)-[:BEFORE]->(b:Event)-[:BEFORE]->(c:Event)
WHERE a.event_type = 'task_start' 
  AND c.event_type = 'task_complete'
RETURN a.name, b.name, c.name, count(*) as pattern_frequency
ORDER BY pattern_frequency DESC
```

### 5. Semantic + Temporal Queries

#### Find semantically similar events in a time window
```cypher
CALL db.idx.vector.queryNodes('event_embeddings', 10, $query_embedding) 
YIELD node, score
WHERE node.start_time >= datetime('2026-01-01T00:00:00Z')
  AND score > 0.8
RETURN node.name, node.start_time, score
ORDER BY score DESC, node.start_time DESC
```

## Integration with Existing Systems

### Migration from facts.jsonl

The migration preserves all existing data while adding temporal reasoning capabilities:

#### Phase 1: Import Facts as Events
```python
def migrate_facts_to_events():
    """Convert temporal facts to events while preserving originals"""
    
    # Read facts.jsonl
    with open('/mnt/ssd/moltbot/clawd/memory/facts.jsonl') as f:
        facts = [json.loads(line) for line in f]
    
    for fact in facts:
        # Create fact node (preserves original structure)
        create_fact_node(fact)
        
        # If fact has temporal significance, create event
        if is_temporal_fact(fact):
            event = {
                'id': f"event-{fact['id']}",
                'name': f"{fact['subject']} {fact['predicate']} {fact['object']}",
                'start_time': fact['valid_from'],
                'end_time': fact['valid_to'],
                'event_type': 'fact_change',
                'confidence': fact['confidence'],
                'source': 'facts_migration'
            }
            create_event_node(event)
            
            # Link fact to event
            create_relationship(f"fact-{fact['id']}", event['id'], 'CORRESPONDS_TO')
```

#### Phase 2: Extract Implicit Events
```python
def extract_implicit_events():
    """Find events implied by facts but not explicitly stored"""
    
    # Decision events from decision facts
    decision_facts = get_facts_by_category('decision')
    for fact in decision_facts:
        create_decision_event(fact)
    
    # State change events from preference updates
    preference_updates = get_preference_changes()
    for change in preference_updates:
        create_state_change_event(change)
    
    # Project milestone events from project facts
    project_facts = get_facts_about_projects()
    for fact in project_facts:
        extract_milestone_events(fact)
```

#### Phase 3: Infer Temporal Relationships
```python
def infer_temporal_relationships():
    """Build temporal relationships from chronological ordering"""
    
    # Get all events sorted by time
    events = get_all_events_sorted()
    
    for i, event in enumerate(events):
        # Find events in temporal proximity
        nearby_events = get_events_in_window(event.start_time, timedelta(hours=24))
        
        for nearby in nearby_events:
            if nearby.id != event.id:
                # Infer Allen relationship
                relationship = determine_allen_relationship(event, nearby)
                create_temporal_relationship(event.id, nearby.id, relationship)
```

### Integration Points

#### 1. facts.jsonl → Event Graph
- **Trigger:** On fact creation/update, check if event should be created
- **Bidirectional:** Events can generate new facts (e.g., "system completed migration at 2026-02-03T15:30:00Z")

#### 2. Memory Router Integration
```python
def temporal_memory_search(query, time_context=None):
    """Enhanced memory router with temporal context"""
    
    if time_context:
        # Add temporal constraints to search
        query += f" during {time_context}"
        
        # Search both facts and events
        fact_results = search_facts(query, temporal_filter=time_context)
        event_results = search_events(query, temporal_filter=time_context)
        
        # Combine and rank results
        return merge_temporal_results(fact_results, event_results)
```

#### 3. Agent Status Integration
```python
def get_agent_temporal_status():
    """Get agent status with temporal context"""
    
    # Recent events by agent
    recent_events = query_recent_events_by_agent()
    
    # Upcoming predicted events
    predicted_events = predict_upcoming_events()
    
    # Pattern analysis
    patterns = analyze_temporal_patterns()
    
    return {
        'recent_activity': recent_events,
        'predicted_activity': predicted_events, 
        'behavioral_patterns': patterns
    }
```

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1)
1. **Database Schema Setup**
   - Create node and relationship types in FalkorDB
   - Set up indices for temporal queries
   - Configure vector indices for semantic search

2. **Basic Event Management**
   - Event creation/update/deletion functions
   - Temporal relationship inference
   - Allen algebra implementation

3. **Migration Tools**
   - facts.jsonl → Event graph migration
   - Data validation and consistency checks

### Phase 2: Query Engine (Week 2)
1. **Temporal Query Language**
   - Natural language → Cypher translation
   - Pre-built query templates
   - Query optimization for temporal operations

2. **CLI Tool Development**
   - `temporal-query` command implementation
   - Integration with existing `graph` command
   - Batch query processing

### Phase 3: Integration & Intelligence (Week 3)
1. **Memory Router Integration**
   - Enhanced federated search with temporal context
   - Cross-system temporal reasoning

2. **Predictive Capabilities**
   - Pattern detection and learning
   - Event prediction based on historical data
   - Anomaly detection in temporal sequences

3. **Agent Integration**
   - Auto-event extraction from agent logs
   - Temporal status reporting
   - Cross-agent temporal coordination

### Phase 4: Advanced Features (Week 4)
1. **Causal Reasoning**
   - Causal relationship inference
   - Impact analysis
   - What-if temporal simulation

2. **Performance Optimization**
   - Query performance tuning
   - Temporal index optimization
   - Memory usage optimization

3. **Visualization & Reporting**
   - Timeline generation
   - Temporal pattern reports
   - Interactive temporal exploration

## CLI Tool Design: `temporal-query`

### Command Structure
```bash
# Basic event queries
temporal-query events --after "2026-01-01" --before "2026-02-01"
temporal-query events --during "last week" --type "task_completion"

# Temporal relationships
temporal-query relationships --event "project launch" --relation "before"
temporal-query sequence --starting-with "morning routine"

# Causal reasoning  
temporal-query causes --effect "system crash" --depth 3
temporal-query impacts --cause "config change" --forward 7d

# Patterns and learning
temporal-query patterns --entity "cody" --timeframe "last month"
temporal-query predict --after "morning coffee" --probability 0.8

# Semantic + temporal
temporal-query similar --to "deployment" --during "last quarter" --limit 10

# Integration queries
temporal-query facts-as-events --category "decision" --since "2026-01-01"
temporal-query agent-timeline --agent "chiron" --date "2026-02-03"
```

### Implementation Sketch
```python
#!/usr/bin/env python3
"""
temporal-query: CLI tool for temporal reasoning over event graphs
"""

import argparse
import json
from datetime import datetime, timedelta
from typing import List, Dict, Optional
import redis
from dateutil import parser as date_parser

class TemporalQuery:
    def __init__(self):
        self.graph = redis.Redis(host='localhost', port=6379, decode_responses=True)
        
    def parse_time_expression(self, expr: str) -> datetime:
        """Parse natural language time expressions"""
        if expr == "now":
            return datetime.now()
        elif expr == "today":
            return datetime.now().replace(hour=0, minute=0, second=0, microsecond=0)
        elif expr.startswith("last "):
            # Handle "last week", "last month", etc.
            period = expr[5:]
            if period == "week":
                return datetime.now() - timedelta(weeks=1)
            elif period == "month":
                return datetime.now() - timedelta(days=30)
            # etc.
        else:
            return date_parser.parse(expr)
    
    def query_events(self, after=None, before=None, event_type=None, during=None):
        """Query events with temporal constraints"""
        
        query_parts = ["MATCH (e:Event)"]
        conditions = []
        
        if after:
            after_dt = self.parse_time_expression(after)
            conditions.append(f"e.start_time >= datetime('{after_dt.isoformat()}')")
            
        if before:
            before_dt = self.parse_time_expression(before)
            conditions.append(f"e.end_time <= datetime('{before_dt.isoformat()}')")
            
        if event_type:
            conditions.append(f"e.event_type = '{event_type}'")
            
        if during:
            # Handle time period expressions
            start, end = self.parse_period(during)
            conditions.append(f"e.start_time >= datetime('{start.isoformat()}')")
            conditions.append(f"e.end_time <= datetime('{end.isoformat()}')")
        
        if conditions:
            query_parts.append("WHERE " + " AND ".join(conditions))
            
        query_parts.append("RETURN e.name, e.start_time, e.end_time, e.event_type")
        query_parts.append("ORDER BY e.start_time")
        
        cypher_query = " ".join(query_parts)
        return self.execute_graph_query(cypher_query)
    
    def query_relationships(self, event_name: str, relation: str):
        """Find temporal relationships for a specific event"""
        
        relation_mapping = {
            'before': 'BEFORE',
            'after': 'AFTER', 
            'during': 'DURING',
            'overlaps': 'OVERLAPS',
            'meets': 'MEETS'
        }
        
        cypher_relation = relation_mapping.get(relation.lower(), relation.upper())
        
        query = f"""
        MATCH (source:Event {{name: '{event_name}'}})-[:{cypher_relation}]->(target:Event)
        RETURN target.name, target.start_time, target.event_type
        ORDER BY target.start_time
        """
        
        return self.execute_graph_query(query)
    
    def query_causal_chain(self, effect_event: str, depth: int = 3):
        """Find causal chain leading to an event"""
        
        query = f"""
        MATCH path = (cause:Event)-[:CAUSED_BY*1..{depth}]->(effect:Event {{name: '{effect_event}'}})
        RETURN [n in nodes(path) | {{name: n.name, time: n.start_time}}] as causal_chain,
               length(path) as chain_length
        ORDER BY chain_length DESC
        """
        
        return self.execute_graph_query(query)
    
    def predict_next_events(self, after_event: str, probability_threshold: float = 0.8):
        """Predict likely next events based on historical patterns"""
        
        # First, find what typically happens after this event
        pattern_query = f"""
        MATCH (trigger:Event {{name: '{after_event}'}})-[:BEFORE]->(next:Event)
        RETURN next.name, next.event_type, count(*) as frequency
        ORDER BY frequency DESC
        LIMIT 10
        """
        
        patterns = self.execute_graph_query(pattern_query)
        
        # Calculate probabilities and filter
        total_occurrences = sum(p['frequency'] for p in patterns)
        predictions = []
        
        for pattern in patterns:
            probability = pattern['frequency'] / total_occurrences
            if probability >= probability_threshold:
                predictions.append({
                    'event': pattern['name'],
                    'probability': probability,
                    'frequency': pattern['frequency']
                })
        
        return predictions
    
    def analyze_patterns(self, entity: str, timeframe: str):
        """Analyze temporal patterns for an entity"""
        
        start_time, end_time = self.parse_period(timeframe)
        
        query = f"""
        MATCH (entity:Entity {{name: '{entity}'}})-[:PARTICIPATES_IN]->(e:Event)
        WHERE e.start_time >= datetime('{start_time.isoformat()}')
          AND e.start_time <= datetime('{end_time.isoformat()}')
        RETURN e.event_type, 
               extract(hour from e.start_time) as hour,
               extract(dayOfWeek from e.start_time) as day_of_week,
               count(*) as frequency
        ORDER BY frequency DESC
        """
        
        return self.execute_graph_query(query)
    
    def execute_graph_query(self, cypher_query: str):
        """Execute Cypher query against FalkorDB"""
        try:
            result = self.graph.execute_command('GRAPH.QUERY', 'temporal_events', cypher_query)
            return self.parse_graph_result(result)
        except Exception as e:
            print(f"Query error: {e}")
            return []
    
    def parse_graph_result(self, result):
        """Parse FalkorDB result format"""
        # FalkorDB returns results in a specific format
        # This would need to be implemented based on actual FalkorDB response format
        return result

def main():
    parser = argparse.ArgumentParser(description='Temporal reasoning queries')
    subparsers = parser.add_subparsers(dest='command', help='Available commands')
    
    # Events subcommand
    events_parser = subparsers.add_parser('events', help='Query events')
    events_parser.add_argument('--after', help='Events after this time')
    events_parser.add_argument('--before', help='Events before this time')  
    events_parser.add_argument('--during', help='Events during this period')
    events_parser.add_argument('--type', help='Filter by event type')
    
    # Relationships subcommand
    rel_parser = subparsers.add_parser('relationships', help='Query temporal relationships')
    rel_parser.add_argument('--event', required=True, help='Event name')
    rel_parser.add_argument('--relation', required=True, help='Temporal relation')
    
    # Causes subcommand
    cause_parser = subparsers.add_parser('causes', help='Query causal chains')
    cause_parser.add_argument('--effect', required=True, help='Effect event')
    cause_parser.add_argument('--depth', type=int, default=3, help='Max chain length')
    
    # Predict subcommand
    predict_parser = subparsers.add_parser('predict', help='Predict next events')
    predict_parser.add_argument('--after', required=True, help='Trigger event')
    predict_parser.add_argument('--probability', type=float, default=0.8, help='Min probability')
    
    # Patterns subcommand  
    pattern_parser = subparsers.add_parser('patterns', help='Analyze patterns')
    pattern_parser.add_argument('--entity', required=True, help='Entity to analyze')
    pattern_parser.add_argument('--timeframe', required=True, help='Time period')
    
    args = parser.parse_args()
    
    tq = TemporalQuery()
    
    if args.command == 'events':
        results = tq.query_events(args.after, args.before, args.type, args.during)
    elif args.command == 'relationships':
        results = tq.query_relationships(args.event, args.relation)
    elif args.command == 'causes':
        results = tq.query_causal_chain(args.effect, args.depth)
    elif args.command == 'predict':
        results = tq.predict_next_events(args.after, args.probability)
    elif args.command == 'patterns':
        results = tq.analyze_patterns(args.entity, args.timeframe)
    else:
        parser.print_help()
        return
    
    # Output results as JSON
    print(json.dumps(results, indent=2, default=str))

if __name__ == '__main__':
    main()
```

## Performance Considerations

### Indexing Strategy
```cypher
-- Temporal indices for fast time-based queries
CREATE INDEX event_start_time FOR (e:Event) ON e.start_time
CREATE INDEX event_end_time FOR (e:Event) ON e.end_time  
CREATE INDEX fact_valid_from FOR (f:Fact) ON f.valid_from
CREATE INDEX fact_valid_to FOR (f:Fact) ON f.valid_to

-- Composite indices for complex queries
CREATE INDEX event_type_time FOR (e:Event) ON (e.event_type, e.start_time)
CREATE INDEX entity_participation FOR ()-[p:PARTICIPATES_IN]-() ON (p.role, p.involvement_level)

-- Vector indices for semantic search
CREATE VECTOR INDEX event_embeddings FOR (e:Event) ON e.embedding
CREATE VECTOR INDEX fact_embeddings FOR (f:Fact) ON f.embedding
```

### Query Optimization
- **Temporal Windows:** Limit unbounded time queries with reasonable defaults
- **Relationship Depth:** Cap recursive relationship traversal (max 5-10 hops)
- **Result Pagination:** Implement SKIP/LIMIT for large result sets
- **Caching:** Cache frequent temporal patterns and relationships

### Storage Estimates
- **Events:** ~1KB per event (with embedding) → 10,000 events = 10MB
- **Relationships:** ~500B per relationship → 100,000 relationships = 50MB  
- **Facts:** ~800B per fact → Current 150 facts = 120KB
- **Indices:** ~20% overhead on data size
- **Total:** ~100MB for mature system (reasonable for FalkorDB)

## Future Enhancements

### Advanced Temporal Reasoning
1. **Fuzzy Temporal Logic:** Handle uncertainty in event timing
2. **Multi-Scale Reasoning:** Events at different temporal granularities
3. **Probabilistic Causation:** Bayesian causal inference over time
4. **Temporal Planning:** Use event patterns for future planning

### Integration Opportunities  
1. **Calendar Integration:** Events from calendar become first-class temporal entities
2. **Log Mining:** Automatic event extraction from system/application logs
3. **Communication Events:** Messages, calls, meetings as temporal events
4. **Task Lifecycle:** Project/task events with automatic state tracking

### Machine Learning
1. **Pattern Discovery:** Unsupervised learning of temporal patterns
2. **Anomaly Detection:** Identify unusual temporal sequences
3. **Prediction Models:** Deep learning for event prediction
4. **Causal Discovery:** Automated causal relationship inference

## Conclusion

This temporal reasoning system provides a comprehensive foundation for storing, querying, and reasoning about events and their temporal relationships. By building on Allen's Interval Algebra and integrating with our existing infrastructure, it enables sophisticated temporal intelligence while maintaining compatibility with current systems.

The phased implementation approach ensures incremental value delivery while building toward a mature temporal reasoning capability that can support advanced agent coordination, predictive analytics, and causal reasoning.

**Next Steps:**
1. Review and approve this design
2. Begin Phase 1 implementation (database schema and migration)
3. Develop `temporal-query` CLI tool prototype
4. Test with real facts.jsonl data migration
5. Iterate based on initial usage patterns

---

*This design document represents comprehensive research into temporal graph modeling and provides a practical implementation path for our memory architecture enhancement.*