use super::ExtractionError;

/// Minimal LLM completion interface for extraction.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full `LlmProvider` + `CompletionRequest` API.
///
/// Uses a boxed future return type to remain dyn-compatible (object-safe).
pub trait ExtractionProvider: Send + Sync {
    /// Send a system + user message to the LLM and return the text response.
    fn complete<'a>(
        &'a self,
        system: &'a str,
        user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    >;

    /// Human-readable provider label for observability (e.g. provider name).
    ///
    /// Defaults to `"unknown"`; bridge implementations should override this
    /// with the actual provider identifier when it is safe to expose.
    fn provider_label(&self) -> String {
        "unknown".to_owned()
    }

    /// Human-readable model label for observability.
    ///
    /// Defaults to `"unknown"`; bridge implementations should override this
    /// with the actual model identifier when it is safe to expose.
    fn model_label(&self) -> String {
        "unknown".to_owned()
    }
}
