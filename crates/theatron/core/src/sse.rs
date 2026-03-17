//! Owned SSE (Server-Sent Events) parser and event source.
//!
//! Replaces the `reqwest-eventsource` git fork with ~100 lines of owned
//! code. Handles the SSE wire protocol: `data:`, `event:`, `id:`,
//! `retry:`, and `:` (comment) fields, delimited by blank lines.

use reqwest::{RequestBuilder, Response, StatusCode};

/// A parsed SSE message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseMessage {
    /// The event type (from `event:` field). Defaults to `"message"`.
    pub event: String,
    /// The data payload (from `data:` field(s), joined with newlines).
    pub data: String,
    /// The last event ID (from `id:` field), if present.
    pub id: Option<String>,
    /// The retry interval in milliseconds (from `retry:` field), if present.
    pub retry: Option<u64>,
}

/// Errors from the SSE connection.
#[derive(Debug)]
pub enum SseError {
    /// HTTP request returned a non-success status code. The full
    /// response is available for body inspection.
    InvalidStatusCode(StatusCode, Response),
    /// Transport or network error.
    Request(reqwest::Error),
}

impl std::fmt::Display for SseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SseError::InvalidStatusCode(status, _) => write!(f, "SSE: HTTP {status}"),
            SseError::Request(e) => write!(f, "SSE request error: {e}"),
        }
    }
}

impl std::error::Error for SseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SseError::Request(e) => Some(e),
            SseError::InvalidStatusCode(..) => None,
        }
    }
}

/// An SSE event source that reads from a streaming HTTP response.
///
/// Cancel-safe: partial reads are buffered in `self`, so dropping
/// the future returned by [`next`](EventSource::next) does not lose data.
pub struct EventSource {
    response: Response,
    buffer: String,
    closed: bool,
}

impl EventSource {
    /// Send the request and begin streaming SSE events.
    ///
    /// # Errors
    ///
    /// Returns [`SseError::Request`] if the HTTP request fails.
    /// Returns [`SseError::InvalidStatusCode`] if the response status is not 2xx.
    pub async fn connect(builder: RequestBuilder) -> Result<Self, SseError> {
        let response = builder.send().await.map_err(SseError::Request)?;
        if !response.status().is_success() {
            return Err(SseError::InvalidStatusCode(response.status(), response));
        }
        Ok(EventSource {
            response,
            buffer: String::new(),
            closed: false,
        })
    }

    /// Read the next SSE message from the stream.
    ///
    /// Returns `None` when the stream ends or after [`close`](EventSource::close)
    /// is called. Only yields events with non-empty `data:` fields (comments
    /// and empty events are silently consumed).
    pub async fn next(&mut self) -> Option<Result<SseMessage, SseError>> {
        if self.closed {
            return None;
        }
        loop {
            if let Some(event) = try_parse(&mut self.buffer) {
                return Some(Ok(event));
            }
            match self.response.chunk().await {
                Ok(Some(bytes)) => {
                    self.buffer.push_str(&String::from_utf8_lossy(&bytes));
                }
                Ok(None) => return None,
                Err(e) => return Some(Err(SseError::Request(e))),
            }
        }
    }

    /// Close the event source. Subsequent calls to [`next`](EventSource::next)
    /// return `None`.
    pub fn close(&mut self) {
        self.closed = true;
    }
}

/// Try to extract one complete SSE event from the buffer. An event is
/// terminated by a blank line (two consecutive line endings).
fn try_parse(buffer: &mut String) -> Option<SseMessage> {
    loop {
        let boundary = find_event_boundary(buffer)?;
        let raw = buffer[..boundary].to_string();

        // Consume the event text and its trailing blank-line delimiter.
        let after = skip_blank_lines(buffer, boundary);
        *buffer = buffer[after..].to_string();

        if let Some(msg) = parse_raw_event(&raw) {
            return Some(msg);
        }
        // WHY: A block of pure comments produces no event. Loop to try
        // the next block rather than returning None (which would trigger
        // a network read even though the buffer may hold more events).
    }
}

/// Find the byte offset of the first event boundary (blank line).
fn find_event_boundary(buffer: &str) -> Option<usize> {
    // Check \r\n\r\n first (longer match) to avoid partial match with \n\n.
    if let Some(pos) = buffer.find("\r\n\r\n") {
        return Some(pos);
    }
    buffer.find("\n\n")
}

/// Return the byte offset past the blank-line delimiter.
fn skip_blank_lines(buffer: &str, boundary: usize) -> usize {
    let tail = &buffer[boundary..];
    let trimmed = tail.trim_start_matches(['\n', '\r']);
    boundary + (tail.len() - trimmed.len())
}

/// Parse a single raw event block into an `SseMessage`. Returns `None`
/// if the block contains no `data:` fields (e.g. a comment-only block).
fn parse_raw_event(raw: &str) -> Option<SseMessage> {
    let mut event_type = None;
    let mut data_parts: Vec<&str> = Vec::new();
    let mut id = None;
    let mut retry = None;

    for line in raw.lines() {
        if line.starts_with(':') {
            continue;
        }

        if let Some((field, value)) = line.split_once(':') {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => data_parts.push(value),
                "event" => event_type = Some(value),
                "id" => id = Some(value),
                "retry" => retry = value.trim().parse().ok(),
                _ => {} // Unknown fields ignored per spec
            }
        }
        // Lines without a colon and not starting with ':' are ignored per spec.
    }

    if data_parts.is_empty() {
        return None;
    }

    Some(SseMessage {
        event: event_type.unwrap_or("message").to_string(),
        data: data_parts.join("\n"),
        id: id.map(ToString::to_string),
        retry,
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions use expect for clarity")]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let mut buf = "data: hello\n\n".to_string();
        let msg = try_parse(&mut buf);
        assert!(msg.is_some(), "should parse a single event");
        let msg = msg.expect("event should parse");
        assert_eq!(msg.event, "message");
        assert_eq!(msg.data, "hello");
        assert!(msg.id.is_none(), "id should be absent");
        assert!(buf.is_empty(), "buffer should be consumed");
    }

    #[test]
    fn parse_multi_line_data() {
        let mut buf = "data: line1\ndata: line2\ndata: line3\n\n".to_string();
        let msg = try_parse(&mut buf).expect("multi-line event should parse");
        assert_eq!(msg.data, "line1\nline2\nline3");
    }

    #[test]
    fn parse_event_and_data_combo() {
        let mut buf = "event: custom\ndata: payload\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event+data combo should parse");
        assert_eq!(msg.event, "custom");
        assert_eq!(msg.data, "payload");
    }

    #[test]
    fn parse_comments_skipped() {
        let mut buf = ": this is a comment\ndata: actual\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event after comment should parse");
        assert_eq!(msg.data, "actual");
    }

    #[test]
    fn parse_comment_only_block_skipped() {
        let mut buf = ": just a comment\n\ndata: real\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event after comment block should parse");
        assert_eq!(msg.data, "real");
        assert!(buf.is_empty(), "buffer should be consumed");
    }

    #[test]
    fn parse_empty_data_field() {
        let mut buf = "data:\n\n".to_string();
        let msg = try_parse(&mut buf).expect("empty data field should parse");
        assert_eq!(msg.data, "");
    }

    #[test]
    fn parse_id_field() {
        let mut buf = "id: 42\ndata: hello\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event with id should parse");
        assert_eq!(msg.id.as_deref(), Some("42"));
    }

    #[test]
    fn parse_retry_field() {
        let mut buf = "retry: 3000\ndata: hello\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event with retry should parse");
        assert_eq!(msg.retry, Some(3000));
    }

    #[test]
    fn parse_invalid_retry_ignored() {
        let mut buf = "retry: abc\ndata: hello\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event with invalid retry should parse");
        assert!(msg.retry.is_none(), "non-numeric retry should be ignored");
    }

    #[test]
    fn parse_crlf_delimiters() {
        let mut buf = "data: hello\r\n\r\n".to_string();
        let msg = try_parse(&mut buf).expect("CRLF-delimited event should parse");
        assert_eq!(msg.data, "hello");
    }

    #[test]
    fn parse_no_data_returns_none_for_block() {
        let mut buf = "event: ping\n\n".to_string();
        let msg = try_parse(&mut buf);
        assert!(msg.is_none(), "events without data should be skipped");
    }

    #[test]
    fn incomplete_event_returns_none() {
        let mut buf = "data: partial".to_string();
        let msg = try_parse(&mut buf);
        assert!(msg.is_none(), "incomplete events should not parse");
        assert_eq!(buf, "data: partial", "buffer should be unchanged");
    }

    #[test]
    fn parse_multiple_events_sequentially() {
        let mut buf = "data: first\n\ndata: second\n\n".to_string();
        let msg1 = try_parse(&mut buf).expect("first event should parse");
        assert_eq!(msg1.data, "first");
        let msg2 = try_parse(&mut buf).expect("second event should parse");
        assert_eq!(msg2.data, "second");
        assert!(buf.is_empty(), "buffer should be fully consumed");
    }

    #[test]
    fn parse_data_without_space_after_colon() {
        let mut buf = "data:nospace\n\n".to_string();
        let msg = try_parse(&mut buf).expect("data without space should parse");
        assert_eq!(msg.data, "nospace");
    }

    #[test]
    fn default_event_type_is_message() {
        let mut buf = "data: test\n\n".to_string();
        let msg = try_parse(&mut buf).expect("default event type should parse");
        assert_eq!(msg.event, "message");
    }

    #[test]
    fn unknown_fields_ignored() {
        let mut buf = "custom: ignored\ndata: kept\n\n".to_string();
        let msg = try_parse(&mut buf).expect("event with unknown fields should parse");
        assert_eq!(msg.data, "kept");
    }

    #[test]
    fn empty_lines_between_events() {
        let mut buf = "data: a\n\n\n\ndata: b\n\n".to_string();
        let a = try_parse(&mut buf).expect("first event should parse");
        assert_eq!(a.data, "a");
        let b = try_parse(&mut buf).expect("second event should parse");
        assert_eq!(b.data, "b");
    }
}
