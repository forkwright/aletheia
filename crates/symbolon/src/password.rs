//! Argon2id password hashing and verification.

use aletheia_koina::secret::SecretString;
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::error::{self, Result};

/// Hash a password with Argon2id (OWASP-recommended defaults).
///
/// Returns the PHC-formatted hash string suitable for storage.
pub(crate) fn hash_password(password: &SecretString) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.expose_secret().as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| {
            error::HashSnafu {
                message: e.to_string(),
            }
            .build()
        })
}

/// Verify a password against a stored Argon2id hash.
///
/// Returns `Ok(true)` if the password matches, `Ok(false)` if it does not.
/// Returns `Err` only if the hash string is malformed.
pub(crate) fn verify_password(password: &SecretString, hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(hash).map_err(|e| {
        error::HashSnafu {
            message: e.to_string(),
        }
        .build()
    })?;

    let argon2 = Argon2::default();
    match argon2.verify_password(password.expose_secret().as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(error::HashSnafu {
            message: e.to_string(),
        }
        .build()),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let pw = secret("correct-horse-battery-staple");
        let hash = hash_password(&pw).unwrap();
        assert!(verify_password(&pw, &hash).unwrap());
    }

    #[test]
    fn wrong_password_rejected() {
        let pw = secret("correct-horse-battery-staple");
        let hash = hash_password(&pw).unwrap();
        let wrong = secret("wrong-password");
        assert!(!verify_password(&wrong, &hash).unwrap());
    }

    #[test]
    fn empty_password_hashes() {
        let pw = secret("");
        let hash = hash_password(&pw).unwrap();
        assert!(verify_password(&pw, &hash).unwrap());
    }

    #[test]
    fn different_passwords_produce_different_hashes() {
        let h1 = hash_password(&secret("password-a")).unwrap();
        let h2 = hash_password(&secret("password-b")).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn same_password_produces_different_hashes_due_to_salt() {
        let pw = secret("same-password");
        let h1 = hash_password(&pw).unwrap();
        let h2 = hash_password(&pw).unwrap();
        assert_ne!(h1, h2);
        assert!(verify_password(&pw, &h1).unwrap());
        assert!(verify_password(&pw, &h2).unwrap());
    }

    #[test]
    fn malformed_hash_returns_error() {
        let pw = secret("password");
        let result = verify_password(&pw, "not-a-valid-hash");
        assert!(result.is_err());
    }
}
