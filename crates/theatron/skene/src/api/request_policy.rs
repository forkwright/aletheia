//! Request policy shared by first-party Aletheia API clients.

use std::fmt;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Default CSRF/request header name used by pylon.
pub const DEFAULT_CSRF_HEADER_NAME: &str = "x-requested-with";

/// Default CSRF/request header value used by pylon.
pub const DEFAULT_CSRF_HEADER_VALUE: &str = "aletheia";

/// Request-header policy advertised by pylon.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPolicy {
    /// CSRF/request-header behavior for state-changing API requests.
    pub csrf: CsrfRequestPolicy,
}

impl RequestPolicy {
    /// Insert every configured first-party request header into `headers`.
    ///
    /// # Errors
    ///
    /// Returns [`RequestPolicyError`] when the policy contains a header name or
    /// value that cannot be represented in HTTP.
    pub fn insert_headers(&self, headers: &mut HeaderMap) -> Result<(), RequestPolicyError> {
        self.csrf.insert_header(headers)
    }

    /// Whether this policy is the bootstrap default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// CSRF/request-header behavior for state-changing API requests.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsrfRequestPolicy {
    /// Whether the server enforces the configured request header.
    pub enabled: bool,
    /// Header name to send on mutating requests.
    pub header_name: String,
    /// Header value to send on mutating requests.
    pub header_value: String, // kanon:ignore RUST/plain-string-secret -- CSRF request marker, not a credential; may contain operator-customized nonce-like text.
}

impl fmt::Debug for CsrfRequestPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsrfRequestPolicy")
            .field("enabled", &self.enabled)
            .field("header_name", &self.header_name)
            .field("header_value", &"[REDACTED]")
            .finish()
    }
}

impl CsrfRequestPolicy {
    /// Insert the CSRF/request header into `headers` when CSRF is enabled.
    ///
    /// # Errors
    ///
    /// Returns [`RequestPolicyError`] when the header name or value is not a
    /// valid HTTP header component.
    pub fn insert_header(&self, headers: &mut HeaderMap) -> Result<(), RequestPolicyError> {
        if !self.enabled {
            return Ok(());
        }

        let name = HeaderName::from_bytes(self.header_name.as_bytes()).map_err(|_source| {
            RequestPolicyError::InvalidHeaderName {
                name: self.header_name.clone(),
            }
        })?;
        let value = HeaderValue::from_str(&self.header_value).map_err(|_source| {
            RequestPolicyError::InvalidHeaderValue {
                name: self.header_name.clone(),
            }
        })?;
        headers.insert(name, value);
        Ok(())
    }
}

impl Default for CsrfRequestPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            header_name: DEFAULT_CSRF_HEADER_NAME.to_owned(),
            header_value: DEFAULT_CSRF_HEADER_VALUE.to_owned(),
        }
    }
}

/// Invalid request-header policy received from config or server metadata.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum RequestPolicyError {
    /// Header name is not valid for HTTP.
    #[snafu(display("invalid request policy header name: {name}"))]
    InvalidHeaderName {
        /// Invalid header name.
        name: String,
    },

    /// Header value is not valid for HTTP.
    #[snafu(display("invalid request policy header value for {name}"))]
    InvalidHeaderValue {
        /// Header whose value was invalid.
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_inserts_bootstrap_header() {
        let mut headers = HeaderMap::new();
        let result = RequestPolicy::default().insert_headers(&mut headers);

        assert!(result.is_ok(), "default policy is valid: {result:?}");

        assert_eq!(
            headers
                .get(DEFAULT_CSRF_HEADER_NAME)
                .and_then(|value| value.to_str().ok()),
            Some(DEFAULT_CSRF_HEADER_VALUE)
        );
    }

    #[test]
    fn custom_policy_inserts_configured_header() {
        let mut headers = HeaderMap::new();
        let policy = RequestPolicy {
            csrf: CsrfRequestPolicy {
                enabled: true,
                header_name: "x-aletheia-csrf".to_owned(),
                header_value: "custom-csrf-value".to_owned(),
            },
        };

        let result = policy.insert_headers(&mut headers);

        assert!(result.is_ok(), "custom policy is valid: {result:?}");

        assert_eq!(
            headers
                .get("x-aletheia-csrf")
                .and_then(|value| value.to_str().ok()),
            Some("custom-csrf-value")
        );
        assert!(headers.get(DEFAULT_CSRF_HEADER_NAME).is_none());
    }

    #[test]
    fn disabled_csrf_policy_inserts_no_header() {
        let mut headers = HeaderMap::new();
        let policy = RequestPolicy {
            csrf: CsrfRequestPolicy {
                enabled: false,
                ..CsrfRequestPolicy::default()
            },
        };

        let result = policy.insert_headers(&mut headers);

        assert!(result.is_ok(), "disabled policy is valid: {result:?}");

        assert!(headers.is_empty());
    }
}
