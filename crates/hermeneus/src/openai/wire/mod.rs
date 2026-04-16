//! Wire types for the OpenAI Chat Completions API.
//!
//! Split by direction: [`request`] serializes outgoing requests,
//! [`response`] deserializes non-streaming responses, [`stream`] parses
//! incremental SSE deltas. Kept crate-private — callers go through
//! [`super::client::OpenAiProvider`].

pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;

pub(crate) use request::ChatCompletionRequest;
pub(crate) use response::ChatCompletionResponse;
pub(crate) use stream::parse_sse_response;
