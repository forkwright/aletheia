use super::*;

fn build_test_graph() -> GraphSnapshot {
    let mut graph = GraphSnapshot::default();

    let entities = [
        ("rust", "Rust", 0.8, 0),
        ("python", "Python", 0.7, 0),
        ("wasm", "WebAssembly", 0.3, 1),
        ("llvm", "LLVM", 0.5, 1),
        ("ml", "Machine Learning", 0.6, 2),
        ("numpy", "NumPy", 0.4, 2),
        ("gpu", "GPU Computing", 0.3, 2),
        ("art", "Digital Art", 0.1, 3),
        ("music", "Algorithmic Music", 0.1, 3),
    ];

    for (id, name, pagerank, community) in &entities {
        graph.add_node(GraphNode {
            entity_id: EntityId::new(*id).expect("valid test id"),
            name: (*name).to_owned(),
            pagerank: *pagerank,
            community: *community,
        });
    }

    let edges = [
        ("rust", "wasm", "compiles_to"),
        ("rust", "python", "interops_with"),
        ("rust", "llvm", "uses"),
        ("python", "ml", "used_for"),
        ("python", "numpy", "depends_on"),
        ("llvm", "wasm", "targets"),
        ("ml", "numpy", "depends_on"),
        ("ml", "gpu", "accelerated_by"),
        ("art", "gpu", "uses"),
        ("music", "ml", "uses"),
    ];

    for (src, dst, rel) in &edges {
        graph.add_edge(src, dst, rel);
    }

    graph
}

#[test]
fn random_walk_visits_reachable_nodes() {
    let graph = build_test_graph();
    let visits = random_walk(
        &graph,
        &["rust".to_owned()],
        &SerendipityConfig::default(),
        42,
    );

    assert!(
        !visits.is_empty(),
        "random walk should visit at least one node"
    );
    assert!(
        !visits.contains_key("rust"),
        "seeds should be excluded from walk results"
    );
}

#[test]
fn random_walk_deterministic_with_same_seed() {
    let graph = build_test_graph();
    let config = SerendipityConfig::default();

    let visits1 = random_walk(&graph, &["rust".to_owned()], &config, 42);
    let visits2 = random_walk(&graph, &["rust".to_owned()], &config, 42);

    assert_eq!(
        visits1, visits2,
        "same RNG seed should produce identical walks"
    );
}

#[test]
fn random_walk_different_seeds_differ() {
    let graph = build_test_graph();
    let config = SerendipityConfig::default();

    let visits1 = random_walk(&graph, &["rust".to_owned()], &config, 42);
    let visits2 = random_walk(&graph, &["rust".to_owned()], &config, 99);

    assert_ne!(
        visits1, visits2,
        "different RNG seeds should produce different walks"
    );
}

#[test]
fn surprise_scores_rank_unfamiliar_entities_higher() {
    let graph = build_test_graph();
    let mut walk_visits = HashMap::new();
    walk_visits.insert("wasm".to_owned(), 5);
    walk_visits.insert("ml".to_owned(), 3);
    walk_visits.insert("art".to_owned(), 2);

    let mut last_access = HashMap::new();
    last_access.insert("wasm".to_owned(), 1.0);
    last_access.insert("ml".to_owned(), 1000.0);
    last_access.insert("art".to_owned(), 5000.0);

    let home = HashSet::from([0_i64]);
    let scores = surprise_scores(&graph, &walk_visits, &home, &last_access);

    assert!(!scores.is_empty(), "should produce surprise scores");

    let art_score = scores.iter().find(|(id, _)| id == "art").map(|(_, s)| *s);
    let wasm_score = scores.iter().find(|(id, _)| id == "wasm").map(|(_, s)| *s);

    assert!(
        art_score > wasm_score,
        "recently-unaccessed cross-community entity should have higher surprise"
    );
}

#[test]
fn score_discoveries_balances_relevance_and_novelty() {
    let graph = build_test_graph();
    let seeds = vec!["rust".to_owned()];
    let distances = bfs_distances(&graph, "rust", 4);
    let config = SerendipityConfig {
        novelty_weight: 0.5,
        ..SerendipityConfig::default()
    };

    let discoveries = score_discoveries(&graph, &seeds, &distances, &config);

    assert!(!discoveries.is_empty(), "should find discoveries from rust");

    for d in &discoveries {
        assert!(d.serendipity_score > 0.0, "score should be positive");
        assert!(d.relevance > 0.0, "relevance should be positive");
    }
}

#[test]
fn high_novelty_weight_favors_distant_communities() {
    let graph = build_test_graph();
    let seeds = vec!["rust".to_owned()];
    let distances = bfs_distances(&graph, "rust", 6);

    let high_novelty = SerendipityConfig {
        novelty_weight: 0.9,
        ..SerendipityConfig::default()
    };

    let high_results = score_discoveries(&graph, &seeds, &distances, &high_novelty);

    assert!(!high_results.is_empty(), "high novelty should find results");

    let high_top_community = high_results[0].community;
    let rust_community = graph.nodes.get("rust").map_or(-1, |n| n.community);
    assert_ne!(
        high_top_community, rust_community,
        "high novelty should prefer cross-community entities"
    );
}

#[test]
fn find_path_between_connected_entities() {
    let graph = build_test_graph();

    let path = find_path(&graph, "rust", "ml", 6);
    assert!(path.is_some(), "should find path from rust to ml");

    let path = path.expect("path exists");
    assert!(path.length >= 2, "rust→ml requires at least 2 hops");
    assert!(!path.edge_labels.is_empty(), "path should have edge labels");
}

#[test]
fn find_path_returns_none_for_disconnected() {
    let mut graph = build_test_graph();
    graph.add_node(GraphNode {
        entity_id: EntityId::new("isolated").expect("valid test id"),
        name: "Isolated Node".to_owned(),
        pagerank: 0.0,
        community: 99,
    });

    let path = find_path(&graph, "rust", "isolated", 6);
    assert!(path.is_none(), "should not find path to isolated node");
}

#[test]
fn find_path_respects_max_depth() {
    let graph = build_test_graph();

    let path = find_path(&graph, "rust", "art", 1);
    assert!(path.is_none(), "should not find rust→art within 1 hop");

    let path = find_path(&graph, "rust", "art", 6);
    assert!(path.is_some(), "should find rust→art within 6 hops");
}

#[test]
fn explore_from_returns_interesting_entities() {
    let graph = build_test_graph();
    let config = SerendipityConfig::default();

    let paths = explore_from(&graph, "rust", &config);

    assert!(!paths.is_empty(), "should find exploration paths from rust");

    for path in &paths {
        assert!(path.length > 0, "path should have non-zero length");
        assert!(!path.nodes.is_empty(), "path should have at least one node");
    }
}

#[test]
fn explore_from_cross_community_paths_rank_higher() {
    let graph = build_test_graph();
    let config = SerendipityConfig::default();

    let paths = explore_from(&graph, "rust", &config);

    if paths.len() >= 2 {
        assert!(
            paths[0].interest_score >= paths[1].interest_score,
            "paths should be ranked by interest score"
        );
    }
}

#[test]
fn select_injection_picks_surprising_fact() {
    let discoveries = vec![
        Discovery {
            entity_id: EntityId::new("ml").expect("valid test id"),
            name: "Machine Learning".to_owned(),
            serendipity_score: 0.8,
            relevance: 0.4,
            novelty: 0.9,
            surprise: 0.6,
            graph_distance: Some(2),
            community: 2,
        },
        Discovery {
            entity_id: EntityId::new("wasm").expect("valid test id"),
            name: "WebAssembly".to_owned(),
            serendipity_score: 0.5,
            relevance: 0.8,
            novelty: 0.2,
            surprise: 0.1,
            graph_distance: Some(1),
            community: 1,
        },
    ];

    let mut fact_contents = HashMap::new();
    fact_contents.insert(
        "ml".to_owned(),
        (
            "fact-ml-1".to_owned(),
            "Neural networks can compose music".to_owned(),
        ),
    );

    let config = SerendipityConfig::default();
    let injection = select_injection(&discoveries, &fact_contents, &config);

    assert!(injection.is_some(), "should select an injection");
    let injection = injection.expect("injection exists");
    assert_eq!(injection.fact_id, "fact-ml-1");
    assert!(injection.surprise_score > 0.0);
}

#[test]
fn select_injection_none_when_no_content() {
    let discoveries = vec![Discovery {
        entity_id: EntityId::new("ml").expect("valid test id"),
        name: "Machine Learning".to_owned(),
        serendipity_score: 0.8,
        relevance: 0.4,
        novelty: 0.9,
        surprise: 0.6,
        graph_distance: Some(2),
        community: 2,
    }];

    let fact_contents = HashMap::new();
    let config = SerendipityConfig::default();
    let injection = select_injection(&discoveries, &fact_contents, &config);

    assert!(injection.is_none(), "should return None when no content");
}

#[test]
fn graph_snapshot_add_and_query() {
    let mut graph = GraphSnapshot::default();
    graph.add_node(GraphNode {
        entity_id: EntityId::new("a").expect("valid test id"),
        name: "A".to_owned(),
        pagerank: 0.5,
        community: 0,
    });
    graph.add_node(GraphNode {
        entity_id: EntityId::new("b").expect("valid test id"),
        name: "B".to_owned(),
        pagerank: 0.3,
        community: 1,
    });
    graph.add_edge("a", "b", "knows");

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.neighbors("a"), vec!["b"]);
    assert_eq!(graph.neighbors("b"), vec!["a"]);
    assert!((graph.max_pagerank - 0.5).abs() < f64::EPSILON);
}

#[test]
fn bfs_distances_correct() {
    let graph = build_test_graph();
    let distances = bfs_distances(&graph, "rust", 4);

    assert_eq!(
        distances.get("rust").copied(),
        Some(0),
        "source should be 0"
    );
    assert_eq!(
        distances.get("python").copied(),
        Some(1),
        "python is 1 hop from rust"
    );
    assert_eq!(
        distances.get("wasm").copied(),
        Some(1),
        "wasm is 1 hop from rust"
    );
    assert_eq!(
        distances.get("ml").copied(),
        Some(2),
        "ml is 2 hops from rust"
    );
}

#[test]
fn explored_path_serde_roundtrip() {
    let path = ExploredPath {
        nodes: vec![
            EntityId::new("a").expect("valid test id"),
            EntityId::new("b").expect("valid test id"),
        ],
        edge_labels: vec!["knows".to_owned()],
        length: 1,
        communities_traversed: 2,
        interest_score: 0.75,
    };
    let json = serde_json::to_string(&path).expect("ExploredPath serialization");
    let back: ExploredPath = serde_json::from_str(&json).expect("ExploredPath deserialization");
    assert_eq!(path.length, back.length, "length should survive roundtrip");
    assert_eq!(
        path.nodes.len(),
        back.nodes.len(),
        "nodes should survive roundtrip"
    );
}

#[test]
fn discovery_serde_roundtrip() {
    let discovery = Discovery {
        entity_id: EntityId::new("test").expect("valid test id"),
        name: "Test Entity".to_owned(),
        serendipity_score: 0.75,
        relevance: 0.5,
        novelty: 0.8,
        surprise: 0.6,
        graph_distance: Some(3),
        community: 2,
    };
    let json = serde_json::to_string(&discovery).expect("Discovery serialization");
    let back: Discovery = serde_json::from_str(&json).expect("Discovery deserialization");
    assert_eq!(
        discovery.entity_id, back.entity_id,
        "entity_id should survive roundtrip"
    );
    assert!(
        (discovery.serendipity_score - back.serendipity_score).abs() < f64::EPSILON,
        "serendipity_score should survive roundtrip"
    );
}
