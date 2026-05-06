//! Bookkeeping provider implementations for the knowledge pipeline.

#[cfg(feature = "gliner")]
mod gliner;

use eidos::bookkeeping::{
    BookkeepingProvider, BookkeepingResult, Extraction, ExtractionSchema, ProviderFailedSnafu,
};

#[cfg(feature = "gliner")]
pub use gliner::{GlinerExtractionProvider, GlinerProviderConfig};

use crate::extract::engine::ExtractionEngine;
use crate::extract::refinement::TurnType;
use crate::extract::{ConversationMessage, ExtractionError, ExtractionProvider};

/// LLM-backed bookkeeping provider.
///
/// This is the compatibility implementation for the current extraction path:
/// it delegates to the existing extraction prompt, LLM provider, and parser.
pub struct LlmBookkeepingProvider<'a> {
    engine: &'a ExtractionEngine,
    provider: &'a dyn ExtractionProvider,
}

impl<'a> LlmBookkeepingProvider<'a> {
    /// Create an LLM-backed bookkeeping provider.
    #[must_use]
    pub fn new(engine: &'a ExtractionEngine, provider: &'a dyn ExtractionProvider) -> Self {
        Self { engine, provider }
    }

    /// Extract knowledge using the current default extraction prompt.
    ///
    /// # Errors
    ///
    /// Returns the existing extraction error surface from the LLM call or parser.
    pub(crate) async fn extract_messages(
        &self,
        messages: &[ConversationMessage],
    ) -> Result<Extraction, ExtractionError> {
        let prompt = self.engine.build_prompt(messages);
        let response = self
            .provider
            .complete(&prompt.system, &prompt.user_message)
            .await?;
        self.engine.parse_response(&response)
    }

    /// Extract knowledge with turn-type-specific prompt refinement.
    ///
    /// # Errors
    ///
    /// Returns the existing extraction error surface from the LLM call or parser.
    pub(crate) async fn extract_messages_with_turn_type(
        &self,
        messages: &[ConversationMessage],
        turn_type: TurnType,
    ) -> Result<Extraction, ExtractionError> {
        let prompt = self
            .engine
            .build_prompt_with_turn_type(messages, Some(turn_type));
        let response = self
            .provider
            .complete(&prompt.system, &prompt.user_message)
            .await?;
        self.engine.parse_response(&response)
    }
}

impl BookkeepingProvider for LlmBookkeepingProvider<'_> {
    fn extract_knowledge<'a>(
        &'a self,
        messages: &'a [ConversationMessage],
        _schema: &'a ExtractionSchema,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = BookkeepingResult<Extraction>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.extract_messages(messages).await.map_err(|err| {
                ProviderFailedSnafu {
                    provider: self.name(),
                    operation: "extract_knowledge",
                    message: err.to_string(),
                }
                .build()
            })
        })
    }

    fn name(&self) -> &'static str {
        "llm"
    }
}
