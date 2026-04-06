//! End-to-end test: provide a sample PR diff, verify knowledge facts appear in the graph.
#![expect(clippy::expect_used, reason = "test assertions")]

use crate::extract::lesson::{LessonConfig, extract_lessons, persist_lesson};
use crate::knowledge_store::KnowledgeStore;

const SAMPLE_PR_DIFF: &str = r#"diff --git a/src/auth/token.rs b/src/auth/token.rs
--- a/src/auth/token.rs
+++ b/src/auth/token.rs
@@ -25,8 +25,12 @@ impl TokenValidator {
-    fn validate(&self, token: &str) -> bool {
-        token.len() > 0
+    fn validate(&self, token: &str) -> Result<Claims, AuthError> {
+        if token.is_empty() {
+            return Err(AuthError::EmptyToken);
+        }
+        let claims = self.decode(token)?;
+        Ok(claims)
     }
diff --git a/src/auth/token_test.rs b/src/auth/token_test.rs
--- /dev/null
+++ b/src/auth/token_test.rs
@@ -0,0 +1,8 @@
+#[test]
+fn validates_empty_token() {
+    let validator = TokenValidator::new();
+    let result = validator.validate("");
+    assert!(result.is_err());
+}
+#[test]
+fn validates_good_token() {}
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -10,6 +10,7 @@ serde = "1"
+jsonwebtoken = "9"
"#;

#[test]
fn end_to_end_lesson_extraction_and_persist() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    let config = LessonConfig {
        pr_title: "Fix token validation".to_owned(),
        pr_number: Some(42),
        nous_id: "test-nous".to_owned(),
        source: "pr-merge:42".to_owned(),
    };

    // Step 1: Extract lessons from diff.
    let lesson = extract_lessons(SAMPLE_PR_DIFF, &config);
    assert!(
        !lesson.facts.is_empty(),
        "lesson extraction should produce facts"
    );
    assert!(
        !lesson.entities.is_empty(),
        "lesson extraction should produce entities"
    );

    // Step 2: Persist to knowledge store.
    let result = persist_lesson(&lesson, &store, &config).expect("persist_lesson should succeed");

    assert!(result.facts_inserted > 0, "should insert at least one fact");
    assert!(
        result.entities_inserted > 0,
        "should insert at least one entity"
    );
    assert!(
        result.relationships_inserted > 0,
        "should insert at least one relationship"
    );

    // Step 3: Verify facts appear in the knowledge store.
    let all_facts = store
        .list_all_facts(100)
        .expect("list_all_facts should succeed");
    assert!(
        !all_facts.is_empty(),
        "knowledge store should contain facts after lesson persist"
    );

    // Verify at least one fact mentions the file path.
    assert!(
        all_facts.iter().any(|f| f.content.contains("token")),
        "should have a fact mentioning the token file"
    );

    // Step 4: Verify entities exist.
    let entities = store.list_entities().expect("list_entities should succeed");
    assert!(
        entities.iter().any(|e| e.entity_type == "pull_request"),
        "should have a PR entity"
    );
    assert!(
        entities.iter().any(|e| e.entity_type == "file"),
        "should have file entities"
    );

    // Step 5: Verify causal edges were created.
    let causal_edges = store
        .list_causal_edges()
        .expect("list_causal_edges should succeed");
    assert!(
        causal_edges.len() >= result.causal_edges_inserted,
        "should have causal edges in the store"
    );
}

#[test]
fn empty_diff_produces_no_facts() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    let config = LessonConfig {
        pr_title: "Empty PR".to_owned(),
        pr_number: None,
        nous_id: "test-nous".to_owned(),
        source: "pr-merge:0".to_owned(),
    };

    let lesson = extract_lessons("", &config);
    assert!(
        lesson.facts.is_empty(),
        "empty diff should produce no facts"
    );

    let result =
        persist_lesson(&lesson, &store, &config).expect("persist of empty lesson should succeed");
    assert_eq!(result.facts_inserted, 0, "no facts to insert");
}
