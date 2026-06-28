//! Canary prompt suite: regression testing for dispatch quality (W-12).
//!
//! 25 representative prompts covering 5 capability axes:
//! - Recall: 5 scenarios for knowledge retrieval and fact verification
//! - Tool use: 5 scenarios for tool invocation and error handling
//! - Session lifecycle: 5 scenarios for session management
//! - Knowledge extraction: 5 scenarios for fact extraction and confidence
//! - Conflict resolution: 5 scenarios for balanced analysis and boundaries

use crate::provider::EvalProvider;
use crate::scenario::Scenario;

mod conflict;
mod knowledge;
mod recall;
mod sessions;
mod support;
mod tools;

use conflict::{
    ConflictBalancedAnalysis, ConflictBoundaryAcknowledgment, ConflictDirectCorrection,
    ConflictNuancedPosition, ConflictScopeRedirect,
};
use knowledge::{
    KnowledgeAmbiguousLowConfidence, KnowledgeDetectContradiction, KnowledgeExtractTechnical,
    KnowledgeMetaCategorization, KnowledgeUpdateRevision,
};
use recall::{
    RecallConflictDetection, RecallEmptyKnowledgeGraceful, RecallInsertQueryRoundtrip,
    RecallSemanticSearch, RecallTemporalOrdering,
};
use sessions::{
    SessionCloseReopenRestore, SessionConcurrentOrdering, SessionCreateSendHistory,
    SessionLargeContextDistillation, SessionMultiTurnContext,
};
use tools::{
    ToolFileReadContent, ToolFileWriteReadRoundtrip, ToolInvalidInputError, ToolMultiToolChain,
    ToolWebSearchStructured,
};

/// Provider that returns all canary scenarios for regression testing.
pub struct CanaryProvider;

impl EvalProvider for CanaryProvider {
    fn provide(&self) -> Vec<Box<dyn Scenario>> {
        canary_scenarios()
    }

    // WHY: trait signature is `fn name(&self) -> &str`. CompositeProvider
    // returns a borrowed self.name field, so the trait cannot use 'static.
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature returns &str (borrowed), not &'static str"
    )]
    fn name(&self) -> &str {
        "canary"
    }
}

/// Return all canary scenarios for regression testing dispatch quality.
#[tracing::instrument(skip_all)]
pub fn canary_scenarios() -> Vec<Box<dyn Scenario>> {
    vec![
        // Recall scenarios (5)
        Box::new(RecallInsertQueryRoundtrip),
        Box::new(RecallSemanticSearch),
        Box::new(RecallConflictDetection),
        Box::new(RecallTemporalOrdering),
        Box::new(RecallEmptyKnowledgeGraceful),
        // Tool use scenarios (5)
        Box::new(ToolFileReadContent),
        Box::new(ToolFileWriteReadRoundtrip),
        Box::new(ToolWebSearchStructured),
        Box::new(ToolMultiToolChain),
        Box::new(ToolInvalidInputError),
        // Session lifecycle scenarios (5)
        Box::new(SessionCreateSendHistory),
        Box::new(SessionMultiTurnContext),
        Box::new(SessionCloseReopenRestore),
        Box::new(SessionConcurrentOrdering),
        Box::new(SessionLargeContextDistillation),
        // Knowledge extraction scenarios (5)
        Box::new(KnowledgeExtractTechnical),
        Box::new(KnowledgeDetectContradiction),
        Box::new(KnowledgeUpdateRevision),
        Box::new(KnowledgeAmbiguousLowConfidence),
        Box::new(KnowledgeMetaCategorization),
        // Conflict resolution scenarios (5)
        Box::new(ConflictBalancedAnalysis),
        Box::new(ConflictDirectCorrection),
        Box::new(ConflictBoundaryAcknowledgment),
        Box::new(ConflictNuancedPosition),
        Box::new(ConflictScopeRedirect),
    ]
}

#[cfg(test)]
#[path = "canary_tests.rs"]
mod canary_tests;
