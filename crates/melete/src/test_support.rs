//! Shared test fixtures for melete's own unit tests and its integration-test
//! binaries. Gated behind `test-support` (or plain `test`) so it never ships
//! in a release build.

use hermeneus::types::{Content, Message, Role};

/// Builds a text-only [`Message`] with the given role, matching the default
/// shape every melete test previously constructed by hand.
pub fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: Content::Text(text.to_owned()),
        cache_breakpoint: false,
    }
}
