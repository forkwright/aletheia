# Iterative Memory Retrieval System Design

## Executive Summary

The current memory-router performs single-pass text similarity searches, often missing relevant facts due to vocabulary mismatches and limited query understanding. This design introduces an iterative retrieval system that progressively refines queries, expands search terms, and fuses results from multiple strategies to dramatically improve fact discovery.

**Key Innovation:** Multi-step query evolution that learns from initial results to find better matches on subsequent iterations.

## Current System Analysis

### Limitations of memory-router v1.0

1. **Vocabulary Gap**: Searches for "morning routine" miss facts about "daily habits" 
2. **Single Strategy**: Only word-overlap matching (calculate_similarity)
3. **No Context Learning**: Doesn't use initial results to improve subsequent searches
4. **Static Queries**: No query expansion or refinement
5. **Simple Ranking**: Basic confidence × relevance scoring without result fusion

### Existing Strengths to Preserve

- Multi-source federation (facts.jsonl, MEMORY.md, Letta, MCP)
- Domain-aware routing with confidence thresholds
- Recency weighting for temporal relevance
- Robust error handling and fallback mechanisms

## Algorithm Design: ITERATE Framework

### Core Algorithm: Iterative Retrieval with Adaptive Term Expansion (ITERATE)

```python
def iterate_retrieval(initial_query: str, max_iterations: int = 3) -> List[RankedResult]:
    """
    Iterative retrieval with query expansion and result fusion
    """
    results = []
    query_terms = [initial_query]  # Start with original query
    seen_results = set()  # Deduplication
    confidence_trend = []  # Track improvement
    
    for iteration in range(max_iterations):
        # STEP 1: Expand current iteration's queries
        expanded_queries = expand_queries(query_terms[-1], iteration)
        
        # STEP 2: Execute multiple retrieval strategies in parallel
        iteration_results = []
        for strategy in [exact_match, semantic_match, concept_match, temporal_match]:
            for query in expanded_queries:
                strategy_results = strategy(query, sources=all_sources)
                iteration_results.extend(strategy_results)
        
        # STEP 3: Fuse and rank results from all strategies
        fused_results = fusion_rank(iteration_results, iteration)
        new_results = [r for r in fused_results if r.id not in seen_results]
        
        # STEP 4: Extract concepts for next iteration
        if new_results:
            concept_terms = extract_key_concepts(new_results[:5])  # Top 5 results
            refined_query = refine_query(initial_query, concept_terms, iteration)
            query_terms.append(refined_query)
            
            results.extend(new_results)
            seen_results.update(r.id for r in new_results)
            confidence_trend.append(max(r.confidence for r in new_results))
        
        # STEP 5: Stopping criteria
        if should_stop(confidence_trend, new_results, iteration):
            break
    
    return final_rank(results, initial_query)
```

### STEP 1: Query Expansion Strategies

#### 1.1 Synonym Expansion
```python
def expand_synonyms(query: str) -> List[str]:
    """Expand with synonyms and related terms"""
    expansions = [query]  # Always include original
    
    # Pattern-based synonyms
    synonym_map = {
        'morning routine': ['daily habits', 'morning ritual', 'AM schedule'],
        'SQL optimization': ['query tuning', 'database performance', 'SQL improvement'],
        'communication preferences': ['contact style', 'messaging habits', 'interaction style']
    }
    
    for pattern, synonyms in synonym_map.items():
        if pattern.lower() in query.lower():
            expansions.extend(synonyms)
    
    return expansions
```

#### 1.2 Hierarchical Expansion  
```python
def expand_hierarchical(query: str, iteration: int) -> List[str]:
    """Expand query at different abstraction levels"""
    if iteration == 0:
        return [query]  # Start specific
    elif iteration == 1:
        return [query, generalize_query(query)]  # Add broader terms
    else:
        return [query, generalize_query(query), specialize_query(query)]  # Add narrower terms
    
def generalize_query(query: str) -> str:
    """Make query more general"""
    # "SQL performance optimization" → "database performance"
    # "morning coffee routine" → "morning routine" 
    generalizations = {
        'optimization': 'performance',
        'specific_tool': 'tool',
        'detailed_process': 'process'
    }
    return apply_patterns(query, generalizations)
```

#### 1.3 Context-Aware Expansion
```python
def expand_contextual(query: str, domain: str, recent_results: List[Result]) -> List[str]:
    """Expand based on domain context and recent findings"""
    domain_contexts = {
        'chiron': ['work', 'business', 'SQL', 'dashboard', 'performance'],
        'syl': ['family', 'home', 'personal', 'routine', 'energy'],
        'eiron': ['MBA', 'school', 'academic', 'assignment', 'study']
    }
    
    # Add domain-specific terms
    expansions = [query]
    if domain in domain_contexts:
        for context_term in domain_contexts[domain]:
            expansions.append(f"{query} {context_term}")
    
    # Learn from recent results
    if recent_results:
        key_terms = extract_frequent_terms(recent_results)
        expansions.extend(f"{query} {term}" for term in key_terms[:3])
    
    return expansions
```

### STEP 2: Multi-Strategy Retrieval

#### 2.1 Exact Match Strategy
```python
def exact_match_strategy(query: str, sources: Dict) -> List[Result]:
    """Current memory-router word overlap approach"""
    # Preserve existing calculate_similarity logic
    return current_memory_router_search(query, sources)
```

#### 2.2 Semantic Match Strategy  
```python
def semantic_match_strategy(query: str, sources: Dict) -> List[Result]:
    """Concept-based matching beyond literal words"""
    semantic_patterns = {
        # Communication concepts
        ('prefer', 'contact', 'message'): ('communication', 'interaction', 'reach'),
        # Performance concepts  
        ('slow', 'optimize', 'fast'): ('performance', 'efficiency', 'speed'),
        # Time concepts
        ('morning', 'routine', 'daily'): ('schedule', 'habit', 'regular')
    }
    
    query_concepts = extract_concepts(query)
    matches = []
    
    for source_key, source_content in sources.items():
        content_concepts = extract_concepts(source_content)
        semantic_score = calculate_semantic_overlap(query_concepts, content_concepts, semantic_patterns)
        if semantic_score > threshold:
            matches.append(Result(source_key, content_concepts, semantic_score, 'semantic'))
    
    return matches
```

#### 2.3 Temporal Match Strategy
```python
def temporal_match_strategy(query: str, sources: Dict) -> List[Result]:
    """Boost recent and time-relevant results"""
    time_indicators = extract_time_references(query)  # "recent", "yesterday", "this week"
    
    results = []
    for source in sources:
        base_score = calculate_similarity(query, source.content)
        
        if time_indicators:
            recency_boost = calculate_temporal_relevance(source.timestamp, time_indicators)
            adjusted_score = base_score * (1 + recency_boost)
        else:
            adjusted_score = base_score
            
        results.append(Result(source, adjusted_score, 'temporal'))
    
    return results
```

### STEP 3: Result Fusion and Ranking

#### 3.1 Multi-Strategy Fusion
```python
def fusion_rank(strategy_results: List[List[Result]], iteration: int) -> List[Result]:
    """Combine results from multiple strategies with adaptive weighting"""
    
    # Group results by source ID
    result_groups = {}
    for strategy_name, results in strategy_results:
        for result in results:
            if result.source_id not in result_groups:
                result_groups[result.source_id] = {}
            result_groups[result.source_id][strategy_name] = result
    
    # Fuse scores for each source
    fused_results = []
    for source_id, strategy_scores in result_groups.items():
        # Adaptive strategy weights based on iteration
        weights = calculate_strategy_weights(iteration, strategy_scores)
        
        # Weighted combination
        fused_score = sum(weights[strategy] * score.confidence 
                         for strategy, score in strategy_scores.items())
        
        # Diversity bonus (prefer results from multiple strategies)
        diversity_bonus = len(strategy_scores) * 0.1
        final_score = fused_score + diversity_bonus
        
        fused_results.append(Result(source_id, final_score, strategy_scores))
    
    return sorted(fused_results, key=lambda x: x.confidence, reverse=True)

def calculate_strategy_weights(iteration: int, available_strategies: Dict) -> Dict[str, float]:
    """Adaptive strategy weighting by iteration"""
    base_weights = {
        'exact': 0.4,
        'semantic': 0.3, 
        'temporal': 0.2,
        'concept': 0.1
    }
    
    # Increase semantic weight in later iterations
    if iteration > 0:
        base_weights['semantic'] *= 1.5
        base_weights['concept'] *= 2.0
        base_weights['exact'] *= 0.8
    
    # Normalize to available strategies
    total = sum(base_weights[s] for s in available_strategies if s in base_weights)
    return {s: base_weights[s] / total for s in available_strategies if s in base_weights}
```

### STEP 4: Query Refinement

#### 4.1 Concept Extraction from Results
```python
def extract_key_concepts(top_results: List[Result]) -> List[str]:
    """Extract important terms from successful results"""
    concept_frequency = {}
    
    for result in top_results:
        # Extract nouns and important adjectives
        terms = extract_meaningful_terms(result.content)
        for term in terms:
            concept_frequency[term] = concept_frequency.get(term, 0) + result.confidence
    
    # Return top concepts weighted by result confidence
    sorted_concepts = sorted(concept_frequency.items(), key=lambda x: x[1], reverse=True)
    return [concept for concept, _ in sorted_concepts[:5]]

def refine_query(original: str, learned_concepts: List[str], iteration: int) -> str:
    """Create refined query incorporating learned concepts"""
    if iteration == 1:
        # Add most relevant learned concept
        return f"{original} {learned_concepts[0]}" if learned_concepts else original
    elif iteration == 2:
        # Try alternative phrasing with learned concepts
        return " ".join([original] + learned_concepts[:2])
    else:
        # Broaden with multiple concepts
        return " ".join([original] + learned_concepts[:3])
```

### STEP 5: Intelligent Stopping Criteria

#### 5.1 Convergence Detection
```python
def should_stop(confidence_trend: List[float], new_results: List[Result], 
               iteration: int) -> bool:
    """Determine when to stop iterating"""
    
    # Maximum iterations reached
    if iteration >= 3:
        return True
    
    # No new results found
    if not new_results:
        return True
    
    # Diminishing returns: confidence not improving
    if len(confidence_trend) >= 2:
        improvement = confidence_trend[-1] - confidence_trend[-2]
        if improvement < 0.05:  # Less than 5% improvement
            return True
    
    # Found high-confidence results
    if new_results and max(r.confidence for r in new_results) > 0.9:
        return True
    
    # Found sufficient quantity of good results
    if len(new_results) >= 10 and all(r.confidence > 0.6 for r in new_results[:5]):
        return True
    
    return False
```

## Example: Iterative Improvement in Action

### Query: "morning energy levels"

**Iteration 0 (Original):**
```
Query: "morning energy levels" 
Strategy: exact_match
Results: 2 matches (34% avg confidence)
- "morning routine checklist" (32% confidence)
- "energy management tips" (36% confidence)
```

**Iteration 1 (Synonym Expansion):**
```
Expanded Queries: ["morning energy levels", "daily energy patterns", "AM vitality"]
Strategies: exact_match + semantic_match
New Concepts Learned: ["routine", "patterns", "coffee"]
Results: 6 matches (52% avg confidence)
- Previous 2 results + 4 new findings about morning habits
Refined Query: "morning energy levels routine"
```

**Iteration 2 (Concept Integration):**
```
Expanded Queries: ["morning energy levels routine", "daily energy patterns coffee", "morning routine vitality"]
Strategies: exact_match + semantic_match + temporal_match
New Concepts Learned: ["caffeine", "sleep", "schedule"]
Results: 12 matches (67% avg confidence)
- Previous 6 + 6 new results including sleep impact on energy
Final Query: "morning energy levels routine caffeine schedule"
```

**Final Results Ranked by Fused Score:**
1. "Coffee timing affects energy crash at 3pm" (89% confidence, multi-strategy)
2. "Morning routine: wake 6:30, coffee 7:00, energy peak 9-11am" (85% confidence)
3. "Sleep debt reduces morning energy by 40%" (78% confidence)
[...12 total results]

**Performance Gain:** 
- Single-pass: 2 results, 34% avg confidence
- Iterative: 12 results, 67% avg confidence  
- **+98% improvement** in result quality and quantity

## Integration with Existing memory-router

### Phase 1: Backward-Compatible Enhancement
```bash
# Add iterative flag to existing command
memory-router "query" --iterative          # Enable iterative mode
memory-router "query" --iterative --max-iter 2   # Limit iterations
memory-router "query" --strategy exact     # Force single strategy (current behavior)
```

### Phase 2: Enhanced CLI Options
```bash
memory-router "query" \
  --iterative \
  --strategies exact,semantic,temporal \
  --expansion synonyms,hierarchical \
  --stop-confidence 0.8 \
  --max-iterations 3 \
  --fusion-mode weighted
```

### Phase 3: Intelligent Defaults
- Auto-enable iterative mode for low initial confidence (<40%)
- Domain-specific strategy selection
- Adaptive iteration limits based on result quality

## Implementation Plan

### Phase 1: Foundation (Week 1-2)
**Goal:** Basic iterative framework with synonym expansion

1. **Create iterative-memory-router**
   - New script inheriting current memory-router functionality
   - Add iteration loop with max-iter parameter
   - Implement basic synonym expansion for common terms

2. **Strategy Framework**
   - Abstract strategy interface
   - Migrate existing word-overlap as exact_match_strategy
   - Add simple semantic_match using concept patterns

3. **Result Fusion**
   - Basic weighted combination of strategy scores
   - Deduplication by source ID
   - Simple confidence-based ranking

### Phase 2: Advanced Retrieval (Week 3-4)
**Goal:** Multi-strategy execution with smart stopping

1. **Query Expansion Engine**
   - Hierarchical expansion (general/specific terms)
   - Domain-aware contextual expansion
   - Concept extraction from previous results

2. **Enhanced Strategies**
   - Temporal matching with recency boost
   - Concept matching using semantic patterns
   - Cross-domain concept bridging

3. **Adaptive Fusion**
   - Iteration-aware strategy weighting
   - Diversity bonuses for multi-strategy matches
   - Confidence trend analysis for stopping

### Phase 3: Intelligence & Integration (Week 5-6)
**Goal:** Seamless integration with smart defaults

1. **Smart Defaults**
   - Auto-enable iterative mode based on initial results
   - Domain-specific strategy selection
   - Confidence-based iteration limits

2. **Integration Testing** 
   - Extensive testing with existing query patterns
   - Performance benchmarking vs current system
   - Backward compatibility validation

3. **Knowledge Graph Integration**
   - Connect with MCP Memory for relationship expansion
   - Use fact relationships for concept bridging
   - Leverage entity connections for query refinement

### Phase 4: Learning & Optimization (Week 7-8)
**Goal:** Self-improving system with usage analytics

1. **Query Pattern Learning**
   - Track successful expansion patterns
   - Learn domain-specific synonym mappings
   - Adapt strategy weights based on success rates

2. **Performance Analytics**
   - Result quality metrics (precision/recall)
   - Query success rate tracking
   - Performance optimization based on usage patterns

3. **Advanced Features**
   - Multi-turn query refinement
   - Personalized expansion based on user preferences
   - Cross-session learning from query patterns

## Technical Architecture

### Core Components

```
iterative-memory-router/
├── core/
│   ├── iterator.py           # Main iteration engine  
│   ├── strategies/           # Retrieval strategy implementations
│   │   ├── exact_match.py
│   │   ├── semantic_match.py
│   │   ├── temporal_match.py
│   │   └── concept_match.py
│   ├── expansion/            # Query expansion modules
│   │   ├── synonym_expander.py
│   │   ├── hierarchical_expander.py
│   │   └── contextual_expander.py
│   ├── fusion/               # Result fusion and ranking
│   │   ├── weighted_fusion.py
│   │   └── diversity_ranker.py
│   └── stopping/             # Convergence detection
│       └── confidence_analyzer.py
├── config/                   # Configuration and patterns
│   ├── synonyms.json         # Synonym mappings
│   ├── domain_contexts.json  # Domain-specific terms
│   └── semantic_patterns.json # Concept relationship patterns
├── integration/              # Integration with existing tools
│   ├── memory_router_adapter.py
│   ├── letta_bridge.py
│   └── mcp_connector.py
└── cli/                      # Command-line interface
    ├── iterative_cli.py
    └── analysis_cli.py       # Result analysis tools
```

### Configuration Files

#### synonyms.json
```json
{
  "communication": ["contact", "messaging", "interaction", "reach out"],
  "preferences": ["likes", "style", "approach", "way"],
  "morning": ["AM", "early", "dawn", "wake up"],
  "routine": ["habit", "pattern", "schedule", "ritual"],
  "energy": ["vitality", "alertness", "vigor", "strength"],
  "performance": ["speed", "efficiency", "optimization", "improvement"]
}
```

#### semantic_patterns.json
```json
{
  "communication_cluster": {
    "terms": ["prefer", "contact", "message", "call", "email", "text"],
    "related": ["interaction", "reach", "communication", "style"]
  },
  "performance_cluster": {
    "terms": ["slow", "fast", "optimize", "improve", "efficiency"],
    "related": ["performance", "speed", "quality", "enhancement"]
  }
}
```

### Performance Characteristics

**Time Complexity:** O(k × n × s) where:
- k = number of iterations (typically 2-3)
- n = number of sources in memory
- s = number of strategies (typically 3-4)

**Expected Performance:**
- Single iteration: ~100-200ms (current baseline)  
- Full iterative run: ~300-600ms (2-3x current time)
- Result quality improvement: 50-100% better relevance

**Memory Usage:**
- Intermediate result caching: ~1-5MB per query
- Configuration data: ~100KB (synonyms, patterns)
- Total overhead: <10MB additional memory

## Success Metrics

### Quantitative Metrics
1. **Result Relevance**: Avg confidence score improvement vs single-pass
2. **Result Coverage**: Number of relevant results found per query
3. **Query Success Rate**: % of queries finding high-confidence results (>70%)
4. **Performance**: Response time within 2x of current system

### Qualitative Improvements
1. **Vocabulary Bridging**: Finds results despite word mismatches
2. **Context Awareness**: Better domain-specific result discovery
3. **Progressive Refinement**: Improves result quality over iterations
4. **Stopping Intelligence**: Knows when to stop for optimal efficiency

### Target Performance Goals
- **Result Quality**: 50-100% improvement in average confidence scores
- **Coverage**: 2-3x more relevant results per query  
- **Success Rate**: >80% of queries find high-confidence matches
- **Response Time**: <1 second for most queries

## Risk Mitigation

### Performance Risks
- **Mitigation**: Aggressive caching, parallel strategy execution, early stopping
- **Fallback**: Automatic degradation to single-pass mode if timeout

### Complexity Risks  
- **Mitigation**: Modular architecture, comprehensive testing, gradual rollout
- **Fallback**: Feature flags to disable iterative mode per user/domain

### Integration Risks
- **Mitigation**: Backward-compatible CLI, extensive regression testing
- **Fallback**: Symlink to original memory-router if issues arise

### Quality Risks
- **Mitigation**: Confidence trend analysis, result validation, user feedback
- **Fallback**: Quality gates to fall back to exact-match if fusion fails

## Future Extensions

### Advanced Query Understanding
- Natural language query parsing
- Intent classification (find/compare/summarize)
- Multi-part query decomposition

### Personalization Engine
- User-specific synonym learning
- Personal query pattern optimization  
- Adaptive strategy selection based on user success patterns

### Cross-Agent Learning
- Shared concept mappings between agents
- Cross-domain knowledge transfer
- Collaborative query refinement

### Real-time Learning
- Query success feedback loops
- Automatic synonym discovery from results
- Dynamic pattern learning from user behavior

---

## Conclusion

The iterative memory retrieval system represents a significant evolution beyond simple text matching. By progressively refining queries, expanding search terms, and intelligently fusing results from multiple strategies, we can dramatically improve fact discovery while maintaining the robustness and multi-source federation that makes the current memory-router valuable.

The key innovation is **learning from partial results** to improve subsequent searches - turning memory retrieval from a one-shot lookup into an intelligent exploration process that gets smarter with each iteration.

**Implementation should begin with Phase 1 foundation work, targeting a 50-100% improvement in result quality while maintaining backward compatibility with existing workflows.**