//! TOML decryption pipeline for `enc:`-prefixed config values.
//!
//! [`decrypt_toml_content`] and [`decrypt_toml_value`] implement the fail-closed
//! contract documented on the loader: if the primary key is missing but `enc:`
//! values are present, decryption returns an actionable error listing every
//! affected field rather than warning and continuing.

use tracing::warn;

use crate::encrypt;
use crate::error::{ConfigDecryptSnafu, Result, SerializeTomlSnafu};

/// Decrypt `enc:` values in a parsed TOML value tree in-place.
///
/// Returns an error if encrypted values are found but the decryption key is
/// missing. This prevents the server from silently starting with undecrypted
/// `enc:` values in place of real secrets.
pub(crate) fn decrypt_toml_value(value: &mut toml::Value) -> Result<()> {
    let primary_key = match encrypt::primary_key_path() {
        Some(path) => match encrypt::load_primary_key(&path) {
            Ok(key) => key,
            Err(e) => {
                warn!(error = %e, "failed to load primary key");
                None
            }
        },
        None => None,
    };

    // WHY: collect all encrypted field paths up front so we can return a single
    // actionable error listing every affected field instead of warning per-value
    if primary_key.is_none() {
        let mut enc_paths = Vec::new();
        collect_encrypted_paths(value, String::new(), &mut enc_paths);
        if !enc_paths.is_empty() {
            return Err(ConfigDecryptSnafu {
                fields: enc_paths.join(", "),
            }
            .build());
        }
    }

    encrypt::decrypt_toml_values(value, primary_key.as_ref());
    Ok(())
}

/// Parse TOML content, decrypt any `enc:` values, and serialize back.
///
/// Returns an error if encrypted values are found but the decryption key is
/// missing. This prevents the server from silently starting with undecrypted
/// `enc:` values in place of real secrets.
pub(crate) fn decrypt_toml_content(content: &str) -> Result<String> {
    let mut value: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Ok(content.to_owned()),
    };

    decrypt_toml_value(&mut value)?;

    serialize_decrypted_toml(&value)
}

/// Serialize a decrypted TOML value tree back to a string.
///
/// # Errors
///
/// Returns an error if the value tree cannot be re-serialized to TOML,
/// ensuring callers never receive raw ciphertext in place of decrypted
/// plaintext.
pub(crate) fn serialize_decrypted_toml(value: &toml::Value) -> Result<String> {
    toml::to_string(value).map_err(|e| {
        SerializeTomlSnafu {
            reason: e.to_string(),
        }
        .build()
    })
}

/// Walk a TOML value tree and collect dotted paths of all `enc:`-prefixed strings.
fn collect_encrypted_paths(value: &toml::Value, prefix: String, out: &mut Vec<String>) {
    match value {
        toml::Value::String(s) if encrypt::is_encrypted(s) => {
            out.push(if prefix.is_empty() {
                "<root>".to_owned()
            } else {
                prefix
            });
        }
        toml::Value::Table(table) => {
            for (key, val) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                collect_encrypted_paths(val, path, out);
            }
        }
        toml::Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let path = format!("{prefix}[{i}]");
                collect_encrypted_paths(item, path, out);
            }
        }
        _ => {} // NOTE: scalar TOML values contain no nested encrypted paths
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test harness: seeding fixtures must panic loudly on setup failure"
)]
mod tests {
    use super::*;
    use crate::test_support::EnvJail;

    #[test]
    fn decrypt_toml_content_roundtrips_plain_toml() {
        let content = "[gateway]\nport = 4242\n";
        let result = decrypt_toml_content(content).unwrap_or_else(|e| panic!("decrypt: {e}"));

        assert_eq!(
            result, content,
            "plain TOML without enc: values should round-trip unchanged"
        );
    }

    #[test]
    fn decrypt_toml_content_fails_when_primary_key_missing_and_enc_values_present() {
        let mut jail = EnvJail::new();
        // WHY: both paths to a primary key must be absent to exercise the
        // fail-closed path; otherwise the test may accidentally load a real key.
        jail.remove_env("HOME");
        jail.remove_env("ALETHEIA_PRIMARY_KEY");

        let content = "[gateway.auth]\nsigning_key = \"enc:abc123\"\n";
        let result = decrypt_toml_content(content);

        assert!(
            matches!(result, Err(crate::error::Error::ConfigDecrypt { .. })),
            "missing primary key with enc: values must fail with ConfigDecrypt, not a warning"
        );
    }

    #[test]
    fn decrypt_toml_content_propagates_serialize_error() {
        // WHY: construct an invalid datetime value that the TOML serializer
        // cannot emit, then assert the error path returns Err instead of
        // silently falling back to the original ciphertext.
        let invalid_dt = toml::Value::Datetime(toml::value::Datetime {
            date: Some(toml::value::Date {
                year: 2021,
                month: 13,
                day: 1,
            }),
            time: None,
            offset: None,
        });
        let mut table = toml::map::Map::new();
        table.insert("bad".to_owned(), invalid_dt);
        let value = toml::Value::Table(table);

        let result = serialize_decrypted_toml(&value);
        assert!(
            matches!(result, Err(crate::error::Error::SerializeToml { .. })),
            "non-serializable toml::Value must produce Err(SerializeToml), not Ok or other error"
        );
    }
}
