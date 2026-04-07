//! Integration tests for the krites v2 evaluator.

use std::collections::{BTreeMap, HashMap};

use crate::v2::eval::{evaluate, eval_expr, EvalResult, load_schemas};
use crate::v2::parse::parse;
use crate::v2::schema::{ColumnDef, ColumnType, RelationSchema};
use crate::v2::storage::mem::MemStorage;
use crate::v2::value::Value;

/// Helper to create a test schema.
fn facts_schema() -> RelationSchema {
    RelationSchema::new(
        "facts",
        vec![
            ColumnDef::key("id", ColumnType::String),
            ColumnDef::value("content", ColumnType::String),
            ColumnDef::value("confidence", ColumnType::Float),
            ColumnDef::optional("nous_id", ColumnType::String),
            ColumnDef::value("is_forgotten", ColumnType::Bool).with_default(Value::from(false)),
        ],
    )
}

/// Setup helper: create storage with schema.
fn setup_storage() -> (MemStorage, HashMap<String, RelationSchema>) {
    let storage = MemStorage::new();
    let mut schemas = HashMap::new();
    schemas.insert("facts".to_string(), facts_schema());
    (storage, schemas)
}

#[test]
fn simple_scan() {
    let (storage, schemas) = setup_storage();

    // Insert some facts.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello world', confidence: 0.9}").unwrap();
    let params: BTreeMap<String, Value> = BTreeMap::new();
    
    if let EvalResult::Mutation { rows_affected, .. } = evaluate(&put_stmt, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows_affected, 1);
    } else {
        panic!("expected mutation");
    }

    // Query them back.
    let query = parse("?[id, content] := *facts{id: id, content: content}").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("fact-1"));
        assert_eq!(rows.rows[0][1], Value::from("hello world"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn filter_with_param() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert multiple facts.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello', confidence: 0.9}, {id: 'fact-2', content: 'world', confidence: 0.5}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with filter.
    let query = parse("?[id] := *facts{id: id, content: c}, id = 'fact-1'").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("fact-1"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn filter_comparison() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert multiple facts.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello', confidence: 0.9}, {id: 'fact-2', content: 'world', confidence: 0.5}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with comparison filter.
    let query = parse("?[id] := *facts{id: id, confidence: conf}, conf > 0.6").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("fact-1"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn aggregation_count() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert multiple facts.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello', confidence: 0.9}, {id: 'fact-2', content: 'world', confidence: 0.5}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with count.
    let query = parse("?[count(id)] := *facts{id: id}").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from(2_i64));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn ordering_descending() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert multiple facts.
    let put_stmt = parse(":put facts {id: 'fact-a', content: 'aaa', confidence: 0.5}, {id: 'fact-b', content: 'bbb', confidence: 0.9}, {id: 'fact-c', content: 'ccc', confidence: 0.7}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with descending order by confidence.
    let query = parse("?[id, confidence] := *facts{id: id, confidence: confidence} :order -confidence").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 3);
        // Should be ordered by confidence descending: fact-b (0.9), fact-c (0.7), fact-a (0.5)
        assert_eq!(rows.rows[0][0], Value::from("fact-b"));
        assert_eq!(rows.rows[1][0], Value::from("fact-c"));
        assert_eq!(rows.rows[2][0], Value::from("fact-a"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn limit_results() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert multiple facts.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'a', confidence: 0.9}, {id: 'fact-2', content: 'b', confidence: 0.8}, {id: 'fact-3', content: 'c', confidence: 0.7}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with limit.
    let query = parse("?[id] := *facts{id: id} :limit 2").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 2);
    } else {
        panic!("expected rows");
    }
}

#[test]
fn empty_result() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Query empty relation.
    let query = parse("?[id] := *facts{id: id}").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 0);
        assert!(rows.is_empty());
    } else {
        panic!("expected rows");
    }
}

#[test]
fn parameter_substitution() {
    let (storage, schemas) = setup_storage();

    // Insert a fact.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello', confidence: 0.9}").unwrap();
    let params: BTreeMap<String, Value> = BTreeMap::new();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with parameter.
    let query = parse("?[id, content] := *facts{id: id, content: content}, id = $target_id").unwrap();
    let mut params: BTreeMap<String, Value> = BTreeMap::new();
    params.insert("target_id".to_string(), Value::from("fact-1"));
    
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("fact-1"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn string_operations() {
    let (storage, schemas) = setup_storage();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Insert facts with different content.
    let put_stmt = parse(":put facts {id: 'fact-1', content: 'hello world', confidence: 0.9}, {id: 'fact-2', content: 'goodbye world', confidence: 0.8}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Query with contains filter.
    let query = parse("?[id] := *facts{id: id, content: c}, contains(c, 'hello')").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("fact-1"));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn create_and_use_relation() {
    let storage = MemStorage::new();
    let mut schemas = HashMap::new();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Create a new relation.
    let create_stmt = parse(":create users {id => name: String, age: Int}").unwrap();
    if let EvalResult::Mutation { .. } = evaluate(&create_stmt, &params, &storage, &schemas).unwrap() {
        // Success
    } else {
        panic!("expected mutation");
    }

    // Load schemas from storage.
    schemas = load_schemas(&storage).unwrap();
    assert!(schemas.contains_key("users"));

    // Insert into new relation.
    let put_stmt = parse(":put users {id: 'user-1', name: 'Alice', age: 30}").unwrap();
    if let EvalResult::Mutation { rows_affected, .. } = evaluate(&put_stmt, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows_affected, 1);
    } else {
        panic!("expected mutation");
    }

    // Query the new relation.
    let query = parse("?[name, age] := *users{id: id, name: name, age: age}").unwrap();
    if let EvalResult::Rows(rows) = evaluate(&query, &params, &storage, &schemas).unwrap() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::from("Alice"));
        assert_eq!(rows.rows[0][1], Value::from(30_i64));
    } else {
        panic!("expected rows");
    }
}

#[test]
fn remove_relation() {
    let storage = MemStorage::new();
    let mut schemas = HashMap::new();
    let params: BTreeMap<String, Value> = BTreeMap::new();

    // Create and populate a relation.
    let create_stmt = parse(":create temp {id => value: String}").unwrap();
    evaluate(&create_stmt, &params, &storage, &schemas).unwrap();
    
    schemas = load_schemas(&storage).unwrap();
    
    let put_stmt = parse(":put temp {id: '1', value: 'test'}").unwrap();
    evaluate(&put_stmt, &params, &storage, &schemas).unwrap();

    // Remove the relation.
    let remove_stmt = parse(":remove temp").unwrap();
    if let EvalResult::Mutation { .. } = evaluate(&remove_stmt, &params, &storage, &schemas).unwrap() {
        // Success
    } else {
        panic!("expected mutation");
    }

    // Verify it's gone.
    schemas = load_schemas(&storage).unwrap();
    assert!(!schemas.contains_key("temp"));
}
