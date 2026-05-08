//! HMAC-SHA256 tool-call receipts. Per-session ephemeral key.
//! Active verification on cited receipts; hallucination detection on missing/mismatched.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use regex::Regex;
use sha2::Sha256;
use snafu::Snafu;

const RECEIPT_SEPARATOR: &str = "\x1f"; // ASCII unit separator

/// Ephemeral per-session HMAC signer. The 32-byte key is never persisted,
/// never serialized, and never sent to the model.
#[derive(Debug, Clone)]
pub struct ReceiptSigner {
    key: [u8; 32],
}

impl ReceiptSigner {
    /// Generate a fresh per-session signer from `getrandom`.
    #[must_use]
    pub fn new_session() -> Self {
        Self {
            key: rand::random(),
        }
    }

    /// Sign a tool call. Returns the receipt token (base64url, no padding).
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "32-byte key is always valid for Hmac<Sha256>"
    )]
    pub fn sign(
        &self,
        tool_name: &str,
        args_json: &str,
        result: &str,
        ts: jiff::Timestamp,
    ) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            // WHY: 32-byte key is always valid for Hmac<Sha256>.
            .expect("32-byte key is valid for Hmac<Sha256>");
        mac.update(tool_name.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(args_json.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(result.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(ts.to_string().as_bytes());
        let tag = mac.finalize();
        URL_SAFE_NO_PAD.encode(tag.into_bytes())
    }

    /// Verify receipt against a tuple. Returns Ok if HMAC matches.
    ///
    /// # Errors
    /// Returns [`VerifyError::Malformed`] if the receipt is not valid base64url,
    /// or [`VerifyError::HmacMismatch`] if the HMAC does not match.
    #[expect(
        clippy::expect_used,
        reason = "32-byte key is always valid for Hmac<Sha256>"
    )]
    pub fn verify(
        &self,
        receipt: &str,
        tool_name: &str,
        args_json: &str,
        result: &str,
        ts: jiff::Timestamp,
    ) -> Result<(), VerifyError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(receipt)
            .map_err(|source| VerifyError::Decode { source })?;

        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("32-byte key is valid for Hmac<Sha256>");
        mac.update(tool_name.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(args_json.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(result.as_bytes());
        mac.update(RECEIPT_SEPARATOR.as_bytes());
        mac.update(ts.to_string().as_bytes());

        mac.verify_slice(&decoded)
            .map_err(|_e| VerifyError::HmacMismatch)
    }
}

/// Per-session record of every emitted receipt (in-memory ledger).
#[derive(Debug, Default, Clone)]
pub struct ReceiptLedger {
    entries: Vec<EmittedReceipt>,
}

/// One emitted receipt and the tuple it attests.
#[derive(Debug, Clone)]
pub struct EmittedReceipt {
    /// The receipt token (base64url, no padding).
    pub receipt: String,
    /// Tool name.
    pub tool_name: String,
    /// Arguments JSON at emission time.
    pub args_json: String,
    /// Result text at emission time.
    pub result: String,
    /// Timestamp used for signing.
    pub ts: jiff::Timestamp,
}

impl EmittedReceipt {
    /// Construct a new emitted receipt record.
    #[must_use]
    pub fn new(
        receipt: String,
        tool_name: String,
        args_json: String,
        result: String,
        ts: jiff::Timestamp,
    ) -> Self {
        Self {
            receipt,
            tool_name,
            args_json,
            result,
            ts,
        }
    }
}

impl ReceiptLedger {
    /// Record an emitted receipt in the ledger.
    pub fn record(
        &mut self,
        receipt: String,
        tool_name: String,
        args_json: String,
        result: String,
        ts: jiff::Timestamp,
    ) {
        self.entries.push(EmittedReceipt::new(
            receipt, tool_name, args_json, result, ts,
        ));
    }

    /// Look up a receipt by its token.
    #[must_use]
    pub fn lookup(&self, receipt: &str) -> Option<&EmittedReceipt> {
        self.entries.iter().find(|e| e.receipt == receipt)
    }
}

/// Scan an assistant message for cited receipts and verify each against the ledger.
///
/// # Errors
/// Returns [`HallucinationDetected::HallucinatedReceipt`] if a cited receipt is
/// not present in the ledger, or [`HallucinationDetected::ReceiptInvalid`] if
/// verification fails (e.g. HMAC mismatch).
pub fn scan_and_verify(
    signer: &ReceiptSigner,
    ledger: &ReceiptLedger,
    assistant_text: &str,
) -> Result<(), HallucinationDetected> {
    // WHY: compile-once regex. The pattern matches `[receipt:<base64url-no-pad>]`.
    // Base64url characters are A-Z, a-z, 0-9, -, _. Minimum 32 chars for a 256-bit HMAC.
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        #[expect(clippy::expect_used, reason = "static regex pattern is valid")]
        let re = Regex::new(r"\[receipt:([A-Za-z0-9_-]{32,})\]")
            .expect("receipt citation regex is valid");
        re
    });

    for cap in re.captures_iter(assistant_text) {
        let token = cap
            .get(1)
            .map_or_else(String::new, |m| m.as_str().to_owned());
        let entry =
            ledger
                .lookup(&token)
                .ok_or_else(|| HallucinationDetected::HallucinatedReceipt {
                    receipt: token.clone(),
                })?;

        signer
            .verify(
                &token,
                &entry.tool_name,
                &entry.args_json,
                &entry.result,
                entry.ts,
            )
            .map_err(|source| HallucinationDetected::ReceiptInvalid {
                receipt: token,
                source,
            })?;
    }

    Ok(())
}

/// Error returned when receipt verification fails at the cryptographic level.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum VerifyError {
    /// Receipt missing or malformed (not valid base64url).
    #[snafu(display("receipt missing or malformed"))]
    Malformed,
    /// HMAC mismatch — receipt does not authenticate this tuple.
    #[snafu(display("HMAC mismatch — receipt does not authenticate this tuple"))]
    HmacMismatch,
    /// Base64 decode error.
    #[snafu(display("decode error: {source}"))]
    Decode {
        /// Underlying base64 error.
        source: base64::DecodeError,
    },
}

/// Error returned when the model cites a receipt that cannot be verified,
/// indicating a hallucinated or corrupted tool call reference.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum HallucinationDetected {
    /// Model cited a receipt not present in the ledger — fabricated tool call.
    #[snafu(display("model cited receipt {receipt} not present in ledger — fabricated tool call"))]
    HallucinatedReceipt {
        /// The receipt token cited by the model.
        receipt: String,
    },
    /// Receipt present in ledger but verification failed.
    #[snafu(display("receipt {receipt} verification failed: {source}"))]
    ReceiptInvalid {
        /// The receipt token.
        receipt: String,
        /// Underlying verification error.
        source: VerifyError,
    },
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_signer() -> ReceiptSigner {
        ReceiptSigner::new_session()
    }

    fn make_ledger() -> ReceiptLedger {
        ReceiptLedger::default()
    }

    #[test]
    fn signer_produces_verifiable_receipt() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", r#"{"path":"/tmp/a"}"#, "hello", ts);
        assert!(
            signer
                .verify(&token, "read_file", r#"{"path":"/tmp/a"}"#, "hello", ts)
                .is_ok()
        );
    }

    #[test]
    fn modified_args_invalidates_receipt() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", r#"{"path":"/tmp/a"}"#, "hello", ts);
        let err = signer
            .verify(&token, "read_file", r#"{"path":"/tmp/b"}"#, "hello", ts)
            .unwrap_err();
        assert!(matches!(err, VerifyError::HmacMismatch));
    }

    #[test]
    fn modified_result_invalidates_receipt() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", r#"{"path":"/tmp/a"}"#, "hello", ts);
        let err = signer
            .verify(&token, "read_file", r#"{"path":"/tmp/a"}"#, "world", ts)
            .unwrap_err();
        assert!(matches!(err, VerifyError::HmacMismatch));
    }

    #[test]
    fn modified_timestamp_invalidates_receipt() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", r#"{"path":"/tmp/a"}"#, "hello", ts);
        let err = signer
            .verify(
                &token,
                "read_file",
                r#"{"path":"/tmp/a"}"#,
                "hello",
                ts + jiff::SignedDuration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(err, VerifyError::HmacMismatch));
    }

    #[test]
    fn cross_session_receipt_is_rejected() {
        let signer_a = make_signer();
        let signer_b = make_signer();
        let ts = jiff::Timestamp::now();
        let token = signer_a.sign("read_file", r#"{"path":"/tmp/a"}"#, "hello", ts);
        let err = signer_b
            .verify(&token, "read_file", r#"{"path":"/tmp/a"}"#, "hello", ts)
            .unwrap_err();
        assert!(matches!(err, VerifyError::HmacMismatch));
    }

    #[test]
    fn malformed_receipt_decode_fails() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let err = signer
            .verify("!!!bad!!!", "read_file", "args", "result", ts)
            .unwrap_err();
        assert!(matches!(err, VerifyError::Decode { .. }));
    }

    #[test]
    fn scan_without_citations_succeeds() {
        let signer = make_signer();
        let ledger = make_ledger();
        assert!(scan_and_verify(&signer, &ledger, "There is no citation here.").is_ok());
    }

    #[test]
    fn scan_with_valid_citation_succeeds() {
        let signer = make_signer();
        let mut ledger = make_ledger();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", "args", "result", ts);
        ledger.record(
            token.clone(),
            "read_file".to_owned(),
            "args".to_owned(),
            "result".to_owned(),
            ts,
        );
        let msg = format!("I used the tool earlier [receipt:{token}].");
        assert!(scan_and_verify(&signer, &ledger, &msg).is_ok());
    }

    #[test]
    fn scan_with_unknown_citation_fails() {
        let signer = make_signer();
        let ledger = make_ledger();
        let msg = "I used the tool earlier [receipt:abc123abc123abc123abc123abc123abc123abc123].";
        let err = scan_and_verify(&signer, &ledger, msg).unwrap_err();
        assert!(matches!(
            err,
            HallucinationDetected::HallucinatedReceipt { .. }
        ));
    }

    #[test]
    fn scan_with_tampered_citation_fails() {
        let signer = make_signer();
        let mut ledger = make_ledger();
        let ts = jiff::Timestamp::now();
        let token = signer.sign("read_file", "args", "result", ts);
        // Record the receipt but with different args/result so verify fails
        ledger.record(
            token.clone(),
            "read_file".to_owned(),
            "tampered_args".to_owned(),
            "tampered_result".to_owned(),
            ts,
        );
        let msg = format!("I used the tool earlier [receipt:{token}].");
        let err = scan_and_verify(&signer, &ledger, &msg).unwrap_err();
        assert!(matches!(err, HallucinationDetected::ReceiptInvalid { .. }));
    }

    #[test]
    fn receipt_isolated_to_own_session() {
        let signer_a = make_signer();
        let mut ledger_a = make_ledger();
        let signer_b = make_signer();
        let ledger_b = make_ledger();

        let ts = jiff::Timestamp::now();
        let token = signer_a.sign("read_file", "args", "result", ts);
        ledger_a.record(
            token.clone(),
            "read_file".to_owned(),
            "args".to_owned(),
            "result".to_owned(),
            ts,
        );

        // Ledger B does not have the receipt
        let msg = format!("I used the tool earlier [receipt:{token}].");
        let err = scan_and_verify(&signer_b, &ledger_b, &msg).unwrap_err();
        assert!(matches!(
            err,
            HallucinationDetected::HallucinatedReceipt { .. }
        ));
    }
}
