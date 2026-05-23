const KRITES_LIB: &str = include_str!("../src/lib.rs");
const ARCHITECTURE: &str = include_str!("../../../docs/ARCHITECTURE.md");
const LEXICON: &str = include_str!("../../../docs/lexicon.md");
const GLOSSARY: &str = include_str!("../../../docs/glossary.md");

#[test]
fn krites_docs_name_datalog_boundary() {
    assert!(
        KRITES_LIB.contains("Datalog query satisfaction"),
        "krites crate docs must describe Datalog satisfaction, not generic judging"
    );
    assert!(
        KRITES_LIB.contains("does not evaluate agent behavior"),
        "krites crate docs must cross-reference dokimion for agent evaluation"
    );
}

#[test]
fn architecture_docs_separate_krites_from_dokimion() {
    assert!(
        ARCHITECTURE.contains("`krites` is the embedded Datalog and graph query engine"),
        "architecture docs must name the krites query-engine boundary"
    );
    assert!(
        ARCHITECTURE.contains("`dokimion` is the behavioral and cognitive evaluation runner"),
        "architecture docs must name the dokimion evaluation boundary"
    );
}

#[test]
fn lexicon_and_glossary_separate_query_engine_from_evaluation() {
    assert!(
        LEXICON.contains("judges Datalog query satisfaction, not agent behavior"),
        "lexicon must qualify the krites judge metaphor"
    );
    assert!(
        LEXICON.contains("Behavioral and cognitive evaluation framework"),
        "lexicon must make dokimion discoverable as the evaluation crate"
    );
    assert!(
        GLOSSARY.contains("not agent behavior"),
        "glossary must qualify the krites judge metaphor"
    );
    assert!(
        GLOSSARY.contains("agent evaluation lives in `dokimion`"),
        "glossary must cross-reference dokimion from krites"
    );
}
