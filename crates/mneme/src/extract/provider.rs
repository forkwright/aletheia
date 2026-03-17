use super::ExtractionError;

/// Minimal LLM completion interface for extraction.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full `LlmProvider` + `CompletionRequest` API.
///
/// Uses a boxed future return type to remain dyn-compatible (object-safe).
pub trait ExtractionProvider: Send + Sync {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    >;
}
