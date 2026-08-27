//! HMAC-SHA256 tool-call receipts. Per-session ephemeral key.
//! Active verification on cited receipts; hallucination detection on missing/mismatched.

use std::collections::{HashMap, VecDeque};

use hmac::{Hmac, KeyInit, Mac};
use regex::Regex;
use sha2::Sha256;
use snafu::Snafu;

use crate::types::RedactionPolicy;

const RECEIPT_SEPARATOR: &str = "\x1f"; // ASCII unit separator
const RECEIPT_V2_DOMAIN: &[u8] = b"aletheia.tool-receipt.v2";
const RECEIPT_V2_INPUT_DOMAIN: &[u8] = b"aletheia.tool-receipt.v2.input";
const RECEIPT_V2_OUTPUT_DOMAIN: &[u8] = b"aletheia.tool-receipt.v2.output";

/// Version of the tuple authenticated by a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptVersion {
    /// Legacy receipt over durable display arguments/result text.
    V1,
    /// Receipt over keyed commitments to the actual prepared input/output plus the
    /// approval and redaction decisions that admitted the call.
    V2,
}

/// Version-two receipt tuple.
///
/// Raw executor inputs and outputs are intentionally absent. Their
/// session-keyed commitments bind the receipt to what ran without turning the
/// ledger into an offline oracle for low-entropy credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptAttestationV2 {
    /// Provider tool-use identity, binding an approval decision to one call
    /// rather than merely to every invocation of the same registered tool.
    pub tool_use_id: String,
    /// Registered tool identity.
    pub tool_name: String,
    /// Session-keyed commitment to canonical JSON after vault,
    /// file-reference, and declared path-string expansion.
    ///
    /// This attests the exact input passed to the executor, not filesystem
    /// inode identity across the final validate-to-I/O race.
    pub input_commitment: String,
    /// Session-keyed commitment to the exact bounded result content delivered
    /// to the model, before the receipt citation is appended.
    pub output_commitment: String,
    /// Effective approval requirement derived for this concrete call.
    pub approval_requirement: String,
    /// Approval-gate or automatic decision that admitted execution.
    pub approval_outcome: String,
    /// Declared redaction-policy identity applied to durable surfaces.
    pub redaction_policy: RedactionPolicy,
    /// Timestamp included in the authenticated tuple.
    pub ts: jiff::Timestamp,
}

fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize_json).collect())
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                if let Some(value) = map.get(key) {
                    canonical.insert(key.clone(), canonicalize_json(value));
                }
            }
            serde_json::Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

fn update_v2_field(mac: &mut Hmac<Sha256>, value: &str) {
    let len = u64::try_from(value.len()).unwrap_or(u64::MAX);
    mac.update(&len.to_be_bytes());
    mac.update(value.as_bytes());
}

fn update_v2_policy(mac: &mut Hmac<Sha256>, policy: &RedactionPolicy) {
    match policy {
        RedactionPolicy::None => mac.update(&[0]),
        RedactionPolicy::Full => mac.update(&[1]),
        RedactionPolicy::Fields(fields) => {
            mac.update(&[2]);
            mac.update(
                &u64::try_from(fields.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for field in fields {
                update_v2_field(mac, field);
            }
        }
    }
}

/// Ephemeral per-session HMAC signer. The 32-byte key is never persisted,
/// never serialized, and never sent to the model.
#[derive(Clone)]
pub struct ReceiptSigner {
    key: [u8; 32],
}

impl std::fmt::Debug for ReceiptSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiptSigner")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl ReceiptSigner {
    /// Generate a fresh per-session signer from `getrandom`.
    #[must_use]
    pub fn new_session() -> Self {
        Self {
            key: rand::random(),
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "32-byte key is always valid for Hmac<Sha256>"
    )]
    fn commitment(&self, domain: &[u8], value: &serde_json::Value) -> String {
        let canonical = canonicalize_json(value).to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("32-byte key is valid for Hmac<Sha256>");
        mac.update(domain);
        update_v2_field(&mut mac, &canonical);
        let tag = mac.finalize();
        koina::base64::encode_url_safe_no_pad(&tag.into_bytes())
    }

    /// Build a version-two attestation from the exact prepared executor input
    /// and bounded result content.
    ///
    /// The commitments use this signer's ephemeral session key, so the
    /// attestation can be retained on a redacted durable surface without
    /// exposing a reusable plain digest of a secret-bearing payload.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "the attested execution identity is deliberately explicit"
    )]
    pub fn attest_v2(
        &self,
        tool_use_id: impl Into<String>,
        tool_name: impl Into<String>,
        prepared_input: &serde_json::Value,
        output: &serde_json::Value,
        approval_requirement: impl Into<String>,
        approval_outcome: impl Into<String>,
        redaction_policy: RedactionPolicy,
        ts: jiff::Timestamp,
    ) -> ReceiptAttestationV2 {
        ReceiptAttestationV2 {
            tool_use_id: tool_use_id.into(),
            tool_name: tool_name.into(),
            input_commitment: self.commitment(RECEIPT_V2_INPUT_DOMAIN, prepared_input),
            output_commitment: self.commitment(RECEIPT_V2_OUTPUT_DOMAIN, output),
            approval_requirement: approval_requirement.into(),
            approval_outcome: approval_outcome.into(),
            redaction_policy,
            ts,
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
        koina::base64::encode_url_safe_no_pad(&tag.into_bytes())
    }

    /// Verify receipt against a tuple. Returns Ok if HMAC matches.
    ///
    /// # Errors
    /// Returns [`VerifyError::Decode`] if the receipt is not valid base64url,
    /// or [`VerifyError::HmacMismatch`] if the HMAC does not match.
    pub fn verify(
        &self,
        receipt: &str,
        tool_name: &str,
        args_json: &str,
        result: &str,
        ts: jiff::Timestamp,
    ) -> Result<(), VerifyError> {
        // SECURITY(#6847): strict, not the lenient decoder. Under leniency both
        // `<tag>` and `<tag>=` decode to the same bytes and verify against the same
        // MAC, so a receipt the ledger has never seen still passes -- "verified" and
        // "is the receipt we issued" would stop being the same statement.
        let decoded = koina::base64::decode_url_safe_no_pad_strict(receipt)
            .map_err(|source| VerifyError::Decode { source })?;

        #[expect(
            clippy::expect_used,
            reason = "INVARIANT: self.key is always 32 bytes (set by constructor with fixed-size array)"
        )]
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

    /// Sign a version-two attestation. Returns base64url without padding.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "32-byte key is always valid for Hmac<Sha256>"
    )]
    pub fn sign_v2(&self, attestation: &ReceiptAttestationV2) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("32-byte key is valid for Hmac<Sha256>");
        mac.update(RECEIPT_V2_DOMAIN);
        update_v2_field(&mut mac, &attestation.tool_use_id);
        update_v2_field(&mut mac, &attestation.tool_name);
        update_v2_field(&mut mac, &attestation.input_commitment);
        update_v2_field(&mut mac, &attestation.output_commitment);
        update_v2_field(&mut mac, &attestation.approval_requirement);
        update_v2_field(&mut mac, &attestation.approval_outcome);
        update_v2_policy(&mut mac, &attestation.redaction_policy);
        update_v2_field(&mut mac, &attestation.ts.to_string());
        let tag = mac.finalize();
        koina::base64::encode_url_safe_no_pad(&tag.into_bytes())
    }

    /// Verify a version-two receipt against its attestation.
    ///
    /// # Errors
    ///
    /// Returns [`VerifyError::Decode`] for invalid base64url or
    /// [`VerifyError::HmacMismatch`] when any authenticated field differs.
    pub fn verify_v2(
        &self,
        receipt: &str,
        attestation: &ReceiptAttestationV2,
    ) -> Result<(), VerifyError> {
        let decoded = koina::base64::decode_url_safe_no_pad_strict(receipt)
            .map_err(|source| VerifyError::Decode { source })?;
        #[expect(
            clippy::expect_used,
            reason = "32-byte key is always valid for Hmac<Sha256>"
        )]
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("32-byte key is valid for Hmac<Sha256>");
        mac.update(RECEIPT_V2_DOMAIN);
        update_v2_field(&mut mac, &attestation.tool_use_id);
        update_v2_field(&mut mac, &attestation.tool_name);
        update_v2_field(&mut mac, &attestation.input_commitment);
        update_v2_field(&mut mac, &attestation.output_commitment);
        update_v2_field(&mut mac, &attestation.approval_requirement);
        update_v2_field(&mut mac, &attestation.approval_outcome);
        update_v2_policy(&mut mac, &attestation.redaction_policy);
        update_v2_field(&mut mac, &attestation.ts.to_string());
        mac.verify_slice(&decoded)
            .map_err(|_e| VerifyError::HmacMismatch)
    }
}

/// Default maximum number of receipts retained per session.
const DEFAULT_LEDGER_CAPACITY: usize = 500;

/// Per-session record of emitted receipts (in-memory ledger).
#[derive(Debug, Clone)]
pub struct ReceiptLedger {
    entries: HashMap<String, EmittedReceipt>,
    /// WHY: FIFO order of receipt tokens so `record()` can evict the oldest
    /// entry when the capacity cap is reached. (#5677)
    order: VecDeque<String>,
    capacity: usize,
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
    /// The approval-policy outcome that admitted this call (#4835), e.g.
    /// `auto_approved`, `advisory_auto`, or the wire string of a real
    /// approval-gate decision. `None` for legacy entries recorded before
    /// this field existed.
    ///
    /// WHY only this one of the four fields the issue names: `policy_decision`,
    /// `sandbox_mode`, and `executor_identity` are not currently surfaced
    /// back to the dispatch boundary by `organon::subprocess`/`ToolResult`
    /// -- adding always-`None` fields for those would be schema noise, not
    /// provenance, so they are left as a named remainder rather than
    /// fabricated.
    pub approval_outcome: Option<String>,
    /// Authenticated tuple version.
    pub version: ReceiptVersion,
    /// Version-two attestation, absent for legacy entries.
    pub attestation_v2: Option<ReceiptAttestationV2>,
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
        approval_outcome: Option<String>,
    ) -> Self {
        Self {
            receipt,
            tool_name,
            args_json,
            result,
            ts,
            approval_outcome,
            version: ReceiptVersion::V1,
            attestation_v2: None,
        }
    }
}

impl ReceiptLedger {
    /// Create a new ledger with the default capacity cap.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_LEDGER_CAPACITY)
    }

    /// Create a new ledger with a custom capacity cap.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Record an emitted receipt in the ledger.
    pub fn record(
        &mut self,
        receipt: String,
        tool_name: String,
        args_json: String,
        result: String,
        ts: jiff::Timestamp,
        approval_outcome: Option<String>,
    ) {
        let entry = EmittedReceipt::new(
            receipt.clone(),
            tool_name,
            args_json,
            result,
            ts,
            approval_outcome,
        );

        // WHY: receipt tokens are unique; replacing an existing entry must not
        // create a duplicate FIFO slot.
        if self.entries.insert(receipt.clone(), entry).is_some() {
            return;
        }

        self.order.push_back(receipt);

        // WHY: cap the in-memory ledger so long-running sessions do not grow
        // without bound. Eviction is FIFO; recent receipts are retained. (#5677)
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// Record a version-two receipt while retaining only redacted display
    /// payloads alongside the session-keyed attestation.
    pub fn record_v2(
        &mut self,
        receipt: String,
        attestation: ReceiptAttestationV2,
        redacted_args_json: String,
        redacted_result: String,
    ) {
        let entry = EmittedReceipt {
            receipt: receipt.clone(),
            tool_name: attestation.tool_name.clone(),
            args_json: redacted_args_json,
            result: redacted_result,
            ts: attestation.ts,
            approval_outcome: Some(attestation.approval_outcome.clone()),
            version: ReceiptVersion::V2,
            attestation_v2: Some(attestation),
        };
        if self.entries.insert(receipt.clone(), entry).is_some() {
            return;
        }
        self.order.push_back(receipt);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// Look up a receipt by its token.
    #[must_use]
    pub fn lookup(&self, receipt: &str) -> Option<&EmittedReceipt> {
        self.entries.get(receipt)
    }

    /// Number of receipts currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the ledger is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ReceiptLedger {
    fn default() -> Self {
        Self::new()
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

        let verified = match (&entry.version, &entry.attestation_v2) {
            (ReceiptVersion::V2, Some(attestation)) => signer.verify_v2(&token, attestation),
            (ReceiptVersion::V1, None) => signer.verify(
                &token,
                &entry.tool_name,
                &entry.args_json,
                &entry.result,
                entry.ts,
            ),
            _ => Err(VerifyError::HmacMismatch),
        };
        verified.map_err(|source| HallucinationDetected::ReceiptInvalid {
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
    /// HMAC mismatch — receipt does not authenticate this tuple.
    #[snafu(display("HMAC mismatch — receipt does not authenticate this tuple"))]
    HmacMismatch,
    /// Base64 decode error (receipt was not valid base64url).
    #[snafu(display("decode error: {source}"))]
    Decode {
        /// Underlying base64 error.
        source: koina::base64::DecodeError,
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
    fn malformed_base64url_receipt_yields_decode_error() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let err = signer
            .verify("!!!bad!!!", "read_file", "args", "result", ts)
            .unwrap_err();
        assert!(matches!(err, VerifyError::Decode { .. }));
    }

    #[test]
    fn v2_receipt_binds_prepared_input_policy_and_decision() {
        let signer = make_signer();
        let ts = jiff::Timestamp::now();
        let attestation = signer.attest_v2(
            "toolu_1",
            "http_request",
            &serde_json::json!({"headers": {"x": "resolved"}, "url": "https://example.test"}),
            &serde_json::json!("ok"),
            "mandatory",
            "approved",
            RedactionPolicy::Fields(vec!["headers".to_owned()]),
            ts,
        );
        let token = signer.sign_v2(&attestation);
        assert!(signer.verify_v2(&token, &attestation).is_ok());

        let mut changed = attestation.clone();
        changed.approval_outcome = "denied".to_owned();
        assert!(matches!(
            signer.verify_v2(&token, &changed),
            Err(VerifyError::HmacMismatch)
        ));

        let mut changed = attestation.clone();
        changed.tool_use_id = "toolu_2".to_owned();
        assert!(matches!(
            signer.verify_v2(&token, &changed),
            Err(VerifyError::HmacMismatch)
        ));

        let mut changed = attestation.clone();
        changed.redaction_policy = RedactionPolicy::Full;
        assert!(matches!(
            signer.verify_v2(&token, &changed),
            Err(VerifyError::HmacMismatch)
        ));
    }

    #[test]
    fn v2_input_commitment_is_canonical_across_object_key_order() {
        let signer = make_signer();
        let first = signer.attest_v2(
            "toolu_1",
            "tool",
            &serde_json::json!({"b": 2, "a": {"d": 4, "c": 3}}),
            &serde_json::json!("result"),
            "none",
            "auto_approved",
            RedactionPolicy::None,
            jiff::Timestamp::now(),
        );
        let second = signer.attest_v2(
            "toolu_1",
            "tool",
            &serde_json::json!({"a": {"c": 3, "d": 4}, "b": 2}),
            &serde_json::json!("result"),
            "none",
            "auto_approved",
            RedactionPolicy::None,
            first.ts,
        );
        assert_eq!(first.input_commitment, second.input_commitment);
    }

    #[test]
    fn v2_commitments_are_session_keyed_and_signer_debug_hides_the_key() {
        let first_signer = make_signer();
        let second_signer = make_signer();
        let input = serde_json::json!({"pin": "1234"});
        let output = serde_json::json!("ok");
        let ts = jiff::Timestamp::now();
        let first = first_signer.attest_v2(
            "toolu_1",
            "tool",
            &input,
            &output,
            "mandatory",
            "approved",
            RedactionPolicy::Full,
            ts,
        );
        let second = second_signer.attest_v2(
            "toolu_1",
            "tool",
            &input,
            &output,
            "mandatory",
            "approved",
            RedactionPolicy::Full,
            ts,
        );

        assert_ne!(first.input_commitment, second.input_commitment);
        let debug = format!("{first_signer:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("1234"));
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
            None,
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
            None,
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
            None,
        );

        // Ledger B does not have the receipt
        let msg = format!("I used the tool earlier [receipt:{token}].");
        let err = scan_and_verify(&signer_b, &ledger_b, &msg).unwrap_err();
        assert!(matches!(
            err,
            HallucinationDetected::HallucinatedReceipt { .. }
        ));
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "test assertions on collection with previously verified capacity"
    )]
    #[test]
    fn ledger_capacity_evicts_oldest_tokens() {
        let signer = make_signer();
        let mut ledger = ReceiptLedger::with_capacity(3);
        let ts = jiff::Timestamp::now();

        let tokens: Vec<String> = (0..4)
            .map(|i| signer.sign("read_file", &format!("args-{i}"), "result", ts))
            .collect();

        for (i, token) in tokens.iter().enumerate() {
            ledger.record(
                token.clone(),
                "read_file".to_owned(),
                format!("args-{i}"),
                "result".to_owned(),
                ts,
                None,
            );
        }

        assert_eq!(ledger.len(), 3, "ledger should be capped at capacity");
        assert!(
            ledger.lookup(&tokens[0]).is_none(),
            "oldest receipt should be evicted"
        );
        for token in &tokens[1..] {
            assert!(
                ledger.lookup(token).is_some(),
                "recent receipts should still be present"
            );
        }
    }
}
