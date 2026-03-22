//! Tracing layer that redacts sensitive field values before output.
//!
//! Sits in the subscriber stack as the file-output layer when redaction is
//! enabled. Fields matching configured names are replaced or truncated, and
//! all string values are scanned for API key patterns.

use std::collections::HashSet;
use std::fmt as stdfmt;
use std::io::Write;
use std::time::SystemTime;

use tokio::sync::Mutex;

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::redact::redact_sensitive;

/// Redacted field storage attached to spans via extensions.
#[derive(Default, Clone)]
struct RedactedSpanFields {
    fields: Map<String, Value>,
}

/// Tracing layer that redacts sensitive field values in structured JSON output.
///
/// Fields matching `redact_fields` have their values replaced with
/// `[REDACTED]`. Fields matching `truncate_fields` are capped at
/// `truncate_length` characters. All string values are scanned for API key
/// patterns via [`redact_sensitive`].
pub struct RedactingLayer<W> { // kanon:ignore RUST/pub-visibility
    writer: Mutex<W>,
    redact_fields: HashSet<String>,
    truncate_fields: HashSet<String>,
    truncate_length: usize,
}

impl<W> RedactingLayer<W> {
    /// Create a redacting layer that writes JSON to the given writer.
    #[must_use]
    pub fn new( // kanon:ignore RUST/pub-visibility
        writer: W,
        redact_fields: impl IntoIterator<Item = String>,
        truncate_fields: impl IntoIterator<Item = String>,
        truncate_length: usize,
    ) -> Self {
        Self {
            writer: Mutex::new(writer),
            redact_fields: redact_fields.into_iter().collect(),
            truncate_fields: truncate_fields.into_iter().collect(),
            truncate_length,
        }
    }

    fn make_visitor(&self) -> RedactingVisitor<'_> {
        RedactingVisitor {
            fields: Map::new(),
            redact_fields: &self.redact_fields,
            truncate_fields: &self.truncate_fields,
            truncate_length: self.truncate_length,
        }
    }
}

struct RedactingVisitor<'a> {
    fields: Map<String, Value>,
    redact_fields: &'a HashSet<String>,
    truncate_fields: &'a HashSet<String>,
    truncate_length: usize,
}

impl RedactingVisitor<'_> {
    fn redact_string(&self, name: &str, value: &str) -> Value {
        if self.redact_fields.contains(name) {
            return Value::String("[REDACTED]".to_owned());
        }
        let redacted = redact_sensitive(value);
        if self.truncate_fields.contains(name) && redacted.len() > self.truncate_length {
            let truncated: String = redacted.chars().take(self.truncate_length).collect();
            return Value::String(format!("{truncated}[TRUNCATED]"));
        }
        Value::String(redacted)
    }
}

impl Visit for RedactingVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn stdfmt::Debug) {
        let s = format!("{value:?}");
        let redacted = self.redact_string(field.name(), &s);
        self.fields.insert(field.name().to_owned(), redacted);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let redacted = self.redact_string(field.name(), value);
        self.fields.insert(field.name().to_owned(), redacted);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if self.redact_fields.contains(field.name()) {
            self.fields.insert(
                field.name().to_owned(),
                Value::String("[REDACTED]".to_owned()),
            );
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::from(value));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if self.redact_fields.contains(field.name()) {
            self.fields.insert(
                field.name().to_owned(),
                Value::String("[REDACTED]".to_owned()),
            );
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::from(value));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if self.redact_fields.contains(field.name()) {
            self.fields.insert(
                field.name().to_owned(),
                Value::String("[REDACTED]".to_owned()),
            );
        } else {
            self.fields.insert(
                field.name().to_owned(),
                serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number),
            );
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if self.redact_fields.contains(field.name()) {
            self.fields.insert(
                field.name().to_owned(),
                Value::String("[REDACTED]".to_owned()),
            );
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::Bool(value));
        }
    }
}

fn epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

impl<S, W> Layer<S> for RedactingLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: Write + Send + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = self.make_visitor();
        attrs.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(RedactedSpanFields {
                fields: visitor.fields,
            });
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = self.make_visitor();
        values.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            if let Some(existing) = ext.get_mut::<RedactedSpanFields>() {
                existing.fields.extend(visitor.fields);
            } else {
                ext.insert(RedactedSpanFields {
                    fields: visitor.fields,
                });
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = self.make_visitor();
        event.record(&mut visitor);

        let meta = event.metadata();
        let mut output = Map::new();
        output.insert("timestamp".to_owned(), Value::from(epoch_secs()));
        output.insert("level".to_owned(), Value::String(meta.level().to_string()));
        output.insert("target".to_owned(), Value::String(meta.target().to_owned()));
        output.insert("fields".to_owned(), Value::Object(visitor.fields));

        if let Some(scope) = ctx.event_scope(event) {
            let spans: Vec<Value> = scope
                .from_root()
                .map(|span| {
                    let mut span_obj = Map::new();
                    span_obj.insert("name".to_owned(), Value::String(span.name().to_owned()));
                    let ext = span.extensions();
                    if let Some(redacted) = ext.get::<RedactedSpanFields>() {
                        for (k, v) in &redacted.fields {
                            span_obj.insert(k.clone(), v.clone());
                        }
                    }
                    Value::Object(span_obj)
                })
                .collect();
            if !spans.is_empty() {
                output.insert("spans".to_owned(), Value::Array(spans));
            }
        }

        let json = Value::Object(output);
        // WHY: Logging must never crash the application. Write failures
        // in the tracing pipeline are silently dropped. try_lock is used
        // because on_event is a sync fn; lock().await cannot be called here.
        let Ok(mut writer) = self.writer.try_lock() else {
            return;
        };
        let _ = serde_json::to_writer(&mut *writer, &json);
        let _ = writer.write_all(b"\n");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let Ok(mut inner) = self.0.lock() else {
                return Ok(buf.len());
            };
            inner.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn setup(
        redact_fields: &[&str],
        truncate_fields: &[&str],
        truncate_length: usize,
    ) -> (Arc<Mutex<Vec<u8>>>, impl Subscriber) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let layer = RedactingLayer::new(
            TestWriter(Arc::clone(&buffer)),
            redact_fields.iter().map(|s| (*s).to_owned()),
            truncate_fields.iter().map(|s| (*s).to_owned()),
            truncate_length,
        );
        (buffer, tracing_subscriber::registry().with(layer))
    }

    fn output_string(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
        let guard = buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&guard).into_owned()
    }

    #[test]
    fn redacts_sensitive_field_names() {
        let (buf, sub) = setup(&["token", "api_key", "password"], &[], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(token = "sk-secret-value", "check redaction");
        });
        let out = output_string(&buf);
        assert!(
            out.contains("[REDACTED]"),
            "token field value must be redacted: {out}"
        );
        assert!(
            !out.contains("sk-secret-value"),
            "original value must not appear: {out}"
        );
    }

    #[test]
    fn redacts_api_key_patterns_in_any_field() {
        let (buf, sub) = setup(&[], &[], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(
                details = "key is sk-proj-abcdefghij1234567890abcdef end",
                "api check"
            );
        });
        let out = output_string(&buf);
        assert!(
            !out.contains("abcdefghij1234567890"),
            "API key pattern must be scrubbed: {out}"
        );
    }

    #[test]
    fn truncates_long_content_fields() {
        let (buf, sub) = setup(&[], &["body"], 20);
        let long_value = "a".repeat(100);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(body = long_value.as_str(), "truncation check");
        });
        let out = output_string(&buf);
        assert!(
            out.contains("[TRUNCATED]"),
            "long body must be truncated: {out}"
        );
        assert!(
            !out.contains(&"a".repeat(100)),
            "full value must not appear: {out}"
        );
    }

    #[test]
    fn does_not_truncate_short_content() {
        let (buf, sub) = setup(&[], &["body"], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(body = "short text", "no truncation");
        });
        let out = output_string(&buf);
        assert!(
            out.contains("short text"),
            "short value must pass through: {out}"
        );
        assert!(
            !out.contains("[TRUNCATED]"),
            "short value must not be truncated: {out}"
        );
    }

    #[test]
    fn preserves_non_sensitive_fields() {
        let (buf, sub) = setup(&["token"], &[], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(session_id = "abc-123", token = "secret", "mixed fields");
        });
        let out = output_string(&buf);
        assert!(
            out.contains("abc-123"),
            "non-sensitive field must pass through: {out}"
        );
        assert!(
            !out.contains("secret"),
            "sensitive field must be redacted: {out}"
        );
    }

    #[test]
    fn includes_metadata_in_output() {
        let (buf, sub) = setup(&[], &[], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::warn!(count = 42_u64, "warning event");
        });
        let out = output_string(&buf);
        assert!(out.contains("WARN"), "level must be present: {out}");
        assert!(
            out.contains("timestamp"),
            "timestamp must be present: {out}"
        );
        assert!(out.contains("target"), "target must be present: {out}");
    }

    #[test]
    fn redacts_numeric_secret_fields() {
        let (buf, sub) = setup(&["secret"], &[], 200);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(secret = 42_i64, "numeric secret");
        });
        let out = output_string(&buf);
        assert!(
            out.contains("[REDACTED]"),
            "numeric secret must be redacted: {out}"
        );
    }

    #[test]
    fn includes_span_context_in_events() {
        let (buf, sub) = setup(&["token"], &[], 200);
        tracing::subscriber::with_default(sub, || {
            let span = tracing::info_span!("request", token = "secret-tok", req_id = "r1");
            let _guard = span.enter();
            tracing::info!("inside span");
        });
        let out = output_string(&buf);
        assert!(out.contains("request"), "span name must appear: {out}");
        assert!(
            out.contains("r1"),
            "non-sensitive span field must appear: {out}"
        );
        assert!(
            !out.contains("secret-tok"),
            "sensitive span field must be redacted: {out}"
        );
    }
}
