//! Tier-1 (no-LLM) handler registry for deterministic prompt transforms.
//!
//! When the complexity scorer returns [`ModelTier::NoLlm`], the router
//! consults a [`Tier1Registry`] before falling back to a Haiku-class model.
//! Handlers in the registry match against the user's prompt and return a
//! direct response without any LLM call.
//!
//! # Built-in handlers
//!
//! | Handler | Pattern | Example |
//! |---------|---------|---------|
//! | [`RegexReplaceHandler`] | Captures a named `text` group from the prompt and returns the replacement template | "what time is it" → returns a fixed response |
//! | [`ExactMatchHandler`] | Case-insensitive exact-match on the full prompt | "yes" / "no" / "ok" acknowledgements |
//!
//! # Extension
//!
//! Implement [`Tier1Handler`] and register via [`Tier1Registry::register`].
//! Handlers are tried in registration order; the first match wins.

use std::borrow::Cow;

use regex::Regex;
use tracing::debug;

/// A deterministic handler for `NoLlm`-tier prompts.
///
/// Implementors inspect the user's message and either return a direct
/// response string or decline (returning `None`) to let the next handler
/// try. Handlers must be `Send + Sync` for use in multi-threaded runtimes.
pub trait Tier1Handler: Send + Sync {
    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &str;

    /// Try to handle `prompt`. Return `Some(response)` on match, `None`
    /// to pass to the next handler.
    fn try_handle(&self, prompt: &str) -> Option<String>;
}

/// Registry of [`Tier1Handler`]s tried in registration order.
///
/// Call [`Tier1Registry::dispatch`] to find the first matching handler.
/// An empty registry always returns `None`.
pub struct Tier1Registry {
    handlers: Vec<Box<dyn Tier1Handler>>,
}

impl std::fmt::Debug for Tier1Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.handlers.iter().map(|h| h.name()).collect();
        f.debug_struct("Tier1Registry")
            .field("handlers", &names)
            .finish()
    }
}

impl Default for Tier1Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Tier1Registry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Register a handler. Handlers are tried in registration order.
    pub fn register(&mut self, handler: Box<dyn Tier1Handler>) {
        debug!(handler = handler.name(), "registered Tier-1 handler");
        self.handlers.push(handler);
    }

    /// Iterate registered handler names.
    pub fn handler_names(&self) -> impl Iterator<Item = &str> {
        self.handlers.iter().map(|h| h.name())
    }

    /// Try all registered handlers in order. Returns `Some(response)` on
    /// the first match, `None` if no handler matches.
    pub fn dispatch(&self, prompt: &str) -> Option<String> {
        for handler in &self.handlers {
            if let Some(response) = handler.try_handle(prompt) {
                debug!(
                    handler = handler.name(),
                    "Tier-1 handler matched — skipping LLM"
                );
                return Some(response);
            }
        }
        None
    }

    /// Number of registered handlers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

/// Handler that matches a prompt against a regex and returns a template
/// response with named captures substituted.
///
/// Template variables use the form `${name}` and are replaced with the
/// corresponding named capture group from the regex match. If a capture
/// group is absent, the variable is replaced with an empty string.
///
/// # Example
///
/// ```rust
/// use hermeneus::complexity::tier1::{RegexReplaceHandler, Tier1Handler};
///
/// // Match simple arithmetic and return a fixed message.
/// let handler = RegexReplaceHandler::new(
///     "arithmetic-guard",
///     r"(?i)^what is \d+\s*[+\-]\s*\d+\??$",
///     "I can answer arithmetic. Use a calculator for precise results.",
/// ).unwrap();
/// assert!(handler.try_handle("what is 2+2?").is_some());
/// ```
pub struct RegexReplaceHandler {
    name: String,
    pattern: Regex,
    template: String,
}

impl RegexReplaceHandler {
    /// Create a new `RegexReplaceHandler`.
    ///
    /// # Errors
    ///
    /// Returns an error if `pattern` is not a valid regex.
    pub fn new(name: &str, pattern: &str, template: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            name: name.to_owned(),
            pattern: Regex::new(pattern)?,
            template: template.to_owned(),
        })
    }
}

impl Tier1Handler for RegexReplaceHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn try_handle(&self, prompt: &str) -> Option<String> {
        let captures = self.pattern.captures(prompt.trim())?;
        // Substitute ${name} variables from named capture groups.
        let mut result = Cow::Borrowed(self.template.as_str());
        for name in self.pattern.capture_names().flatten() {
            let placeholder = format!("${{{name}}}");
            if result.contains(placeholder.as_str()) {
                let value = captures.name(name).map_or("", |m| m.as_str());
                result = Cow::Owned(result.replace(placeholder.as_str(), value));
            }
        }
        Some(result.into_owned())
    }
}

/// Handler that matches a prompt via case-insensitive exact match and returns
/// a fixed response.
///
/// Whitespace is trimmed before comparison. Useful for single-token
/// acknowledgement turns ("yes", "no", "ok", "done") that should not consume
/// an LLM call.
///
/// # Example
///
/// ```rust
/// use hermeneus::complexity::tier1::{ExactMatchHandler, Tier1Handler};
///
/// let handler = ExactMatchHandler::new("ack-ok", "ok", "Acknowledged.");
/// assert!(handler.try_handle("  OK  ").is_some());
/// assert!(handler.try_handle("ok I think we should").is_none());
/// ```
pub struct ExactMatchHandler {
    name: String,
    expected: String,
    response: String,
}

impl ExactMatchHandler {
    /// Create a new `ExactMatchHandler`.
    #[must_use]
    pub fn new(name: &str, expected: &str, response: &str) -> Self {
        Self {
            name: name.to_owned(),
            expected: expected.to_lowercase(),
            response: response.to_owned(),
        }
    }
}

impl Tier1Handler for ExactMatchHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn try_handle(&self, prompt: &str) -> Option<String> {
        if prompt.trim().to_lowercase() == self.expected {
            Some(self.response.clone())
        } else {
            None
        }
    }
}

/// Build a `Tier1Registry` pre-populated with the default built-in handlers.
///
/// Built-in handlers cover the most common `NoLlm`-tier patterns:
/// single-token acknowledgements (`ok`, `yes`, `no`, etc.) and simple
/// greetings (`hi`, `hello`, `hey`) that appear in the existing
/// `SIMPLE_RESPONSE` regex in the complexity scorer.
///
/// # Errors
///
/// Returns an error if any built-in handler regex is invalid (should never
/// occur in practice since all patterns are compile-time constants).
pub fn default_registry() -> Result<Tier1Registry, regex::Error> {
    let mut reg = Tier1Registry::new();

    // Single-token acknowledgements.
    for (token, reply) in [
        ("ok", "Acknowledged."),
        ("yes", "Noted."),
        ("no", "Understood."),
        ("thanks", "You're welcome."),
        ("thank you", "You're welcome."),
        ("sure", "Acknowledged."),
        ("got it", "Acknowledged."),
        ("lgtm", "Acknowledged."),
        ("ship it", "Acknowledged."),
        ("do it", "Acknowledged."),
        ("go", "Acknowledged."),
        ("go ahead", "Acknowledged."),
        ("k", "Acknowledged."),
        ("yep", "Noted."),
        ("nope", "Understood."),
        ("hi", "Hello!"),
        ("hello", "Hello!"),
        ("hey", "Hey!"),
    ] {
        reg.register(Box::new(ExactMatchHandler::new(
            &format!("ack-{token}"),
            token,
            reply,
        )));
    }

    Ok(reg)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn exact_match_handler_matches_exact() {
        let h = ExactMatchHandler::new("ack-ok", "ok", "Acknowledged.");
        assert_eq!(h.try_handle("ok"), Some("Acknowledged.".to_owned()));
    }

    #[test]
    fn exact_match_handler_trims_whitespace() {
        let h = ExactMatchHandler::new("ack-ok", "ok", "Acknowledged.");
        assert_eq!(h.try_handle("  ok  "), Some("Acknowledged.".to_owned()));
    }

    #[test]
    fn exact_match_handler_is_case_insensitive() {
        let h = ExactMatchHandler::new("ack-ok", "ok", "Ack.");
        assert_eq!(h.try_handle("OK"), Some("Ack.".to_owned()));
    }

    #[test]
    fn exact_match_handler_rejects_non_exact() {
        let h = ExactMatchHandler::new("ack-ok", "ok", "Ack.");
        assert!(h.try_handle("ok I think so").is_none());
    }

    #[test]
    fn regex_replace_handler_matches_and_substitutes() {
        let h = RegexReplaceHandler::new(
            "arithmetic-guard",
            r"(?i)^what is (?P<expr>\d+\s*[+\-]\s*\d+)\??$",
            "Use a calculator for ${expr}.",
        )
        .unwrap();
        let result = h.try_handle("what is 2+2?");
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("2+2"), "expected expr in response: {text}");
    }

    #[test]
    fn regex_replace_handler_returns_none_on_no_match() {
        let h = RegexReplaceHandler::new("guard", r"^fixed$", "Fixed response.").unwrap();
        assert!(h.try_handle("not fixed").is_none());
    }

    #[test]
    fn regex_replace_handler_returns_template_without_captures() {
        let h = RegexReplaceHandler::new("simple", r"^hello$", "Hello back!").unwrap();
        assert_eq!(h.try_handle("hello"), Some("Hello back!".to_owned()));
    }

    #[test]
    fn registry_dispatch_returns_none_when_empty() {
        let reg = Tier1Registry::new();
        assert!(reg.dispatch("anything").is_none());
    }

    #[test]
    fn registry_dispatch_first_match_wins() {
        let mut reg = Tier1Registry::new();
        reg.register(Box::new(ExactMatchHandler::new("a", "ok", "from-a")));
        reg.register(Box::new(ExactMatchHandler::new("b", "ok", "from-b")));
        assert_eq!(reg.dispatch("ok"), Some("from-a".to_owned()));
    }

    #[test]
    fn registry_dispatch_tries_next_on_no_match() {
        let mut reg = Tier1Registry::new();
        reg.register(Box::new(ExactMatchHandler::new("a", "yes", "from-a")));
        reg.register(Box::new(ExactMatchHandler::new("b", "no", "from-b")));
        assert_eq!(reg.dispatch("no"), Some("from-b".to_owned()));
    }

    #[test]
    fn registry_len_and_is_empty() {
        let mut reg = Tier1Registry::new();
        assert!(reg.is_empty());
        reg.register(Box::new(ExactMatchHandler::new("x", "x", "X")));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
    }

    #[test]
    fn registry_handler_names_iterates_in_order() {
        let mut reg = Tier1Registry::new();
        reg.register(Box::new(ExactMatchHandler::new("alpha", "a", "A")));
        reg.register(Box::new(ExactMatchHandler::new("beta", "b", "B")));
        let names: Vec<&str> = reg.handler_names().collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn default_registry_covers_acknowledgement_tokens() {
        let reg = default_registry().unwrap();
        for token in ["ok", "yes", "no", "thanks", "lgtm", "hi", "hello"] {
            assert!(
                reg.dispatch(token).is_some(),
                "default registry should handle token '{token}'"
            );
        }
    }

    #[test]
    fn default_registry_does_not_match_complex_prompts() {
        let reg = default_registry().unwrap();
        assert!(
            reg.dispatch("Please analyze the codebase and suggest improvements")
                .is_none()
        );
    }
}
