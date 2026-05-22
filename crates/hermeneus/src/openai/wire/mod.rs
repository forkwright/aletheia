//! Wire types for OpenAI Chat Completions and Responses APIs.
//!
//! Split by direction: [`request`] serializes outgoing requests,
//! [`response`] deserializes non-streaming responses, [`stream`] parses
//! incremental SSE deltas. Kept crate-private — callers go through
//! [`super::client::OpenAiProvider`].

pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;

pub(crate) use request::{ChatCompletionRequest, ResponsesRequest};
pub(crate) use response::{ChatCompletionResponse, ResponsesResponse};
pub(crate) use stream::{parse_chat_sse_response, parse_responses_sse_response};
