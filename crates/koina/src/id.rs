//! Newtype wrappers for domain identifiers.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::uuid::Uuid;

/// Generate a newtype ID wrapper around a string-like inner type.
///
/// Produces a transparent serde newtype with standard string-like trait
/// implementations. The inner type must implement `AsRef<str>`,
/// `From<String>`, `From<&str>`, and `Into<String>`.
///
/// # Generated API
///
/// - **Derives:** `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Serialize`, `Deserialize`
/// - **Traits:** `Display`, `FromStr`, `AsRef<str>`, `Borrow<str>`, `Deref<Target=str>`,
///   `From<String>`, `From<&str>`, `From<T> for String`, `PartialEq<str>`
/// - **Methods:** `new()`, `into_inner()`, `as_str()`
///
/// # Examples
///
/// ```
/// use koina::newtype_id;
///
/// newtype_id!(
///     /// A widget identifier.
///     pub struct WidgetId(String)
/// );
///
/// let id = WidgetId::new("w-1");
/// assert_eq!(id.as_str(), "w-1");
/// assert_eq!(id.to_string(), "w-1");
/// let back: String = id.into_inner();
/// assert_eq!(back, "w-1");
/// ```
#[macro_export]
macro_rules! newtype_id {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        $vis struct $name($inner);

        impl $name {
            /// Create a new identifier.
            #[must_use]
            $vis fn new(id: impl Into<$inner>) -> Self {
                Self(id.into())
            }

            /// Consume the wrapper and return the inner value.
            #[must_use]
            $vis fn into_inner(self) -> $inner {
                self.0.into()
            }

            /// The underlying string value.
            #[must_use]
            $vis fn as_str(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.0.as_ref())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.into()))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl ::std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                self.0.as_ref()
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s.into())
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.into())
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.0.into()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_ref() == other
            }
        }
    };
}

/// Maximum byte length for a canonical nous identifier.
pub const MAX_NOUS_ID_LEN: usize = 64;

/// Internal identifier namespaces that user-supplied nous IDs must not use.
pub const RESERVED_NOUS_ID_PREFIXES: &[&str] = &["cross:"];

/// Normalize a user-supplied nous identifier into its canonical route-safe form.
///
/// Normalization trims surrounding whitespace, folds ASCII case to lowercase,
/// and maps underscores to hyphens. The resulting ID must be lowercase ASCII
/// alphanumeric plus hyphens, 1-64 bytes, and must not start or end with a
/// hyphen.
///
/// # Errors
///
/// Returns an error if the normalized ID is empty, too long, uses a reserved
/// internal prefix, contains a path separator or other unsupported character,
/// or is not route-safe.
#[must_use = "returns a validated identifier that should not be discarded"]
pub fn normalize_nous_id(id: impl AsRef<str>) -> Result<NousId, IdError> {
    let normalized = canonical_nous_id(id.as_ref());
    validate_id(&normalized, "NousId")?;
    Ok(NousId(normalized))
}

fn canonical_nous_id(id: &str) -> String {
    id.trim()
        .chars()
        .map(|c| {
            if c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// A nous (agent) identifier. Canonical lowercase alphanumeric + hyphens, 1-64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NousId(String);

impl NousId {
    /// Create a new `NousId`, normalizing and validating the format.
    ///
    /// # Errors
    /// Returns an error if the normalized ID is empty, exceeds 64 bytes,
    /// contains unsupported characters, uses a reserved internal prefix, or is
    /// not route-safe.
    #[must_use = "returns a validated identifier that should not be discarded"]
    pub fn new(id: impl Into<String>) -> Result<Self, IdError> {
        normalize_nous_id(id.into())
    }

    /// The underlying string value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NousId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for NousId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for NousId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NousId {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<NousId> for String {
    fn from(id: NousId) -> Self {
        id.0
    }
}

/// A session identifier. UUID v4-based, cryptographically random (128-bit).
///
/// WHY: ULID uses only 80 bits of randomness; UUID v4 provides 122 bits,
/// eliminating any practical guessability risk for session tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(Uuid);

impl SessionId {
    /// Generate a new session ID using UUID v4 (128-bit random).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse from a UUID string (hyphenated), ULID string (Crockford base32),
    /// or the legacy `ses_<24hex>` format produced by aletheia ≤ 0.15.
    ///
    /// Accepts all three for backwards compatibility: newer sessions use UUID,
    /// some historical sessions used ULID (#3101), and pre-ULID sessions
    /// migrated from the `SQLite` v32 schema carry `ses_<24hex>` IDs that this
    /// parser must accept so migrated 0.15 instances stay queryable.
    ///
    /// # Errors
    /// Returns an error if the string matches none of the three formats.
    #[must_use = "returns a parsed session identifier that should not be discarded"]
    pub fn parse(s: &str) -> Result<Self, IdError> {
        // Try UUID first (most common in current code).
        if let Ok(uuid) = Uuid::parse_str(s) {
            return Ok(Self(uuid));
        }
        // Fall back to ULID for legacy compatibility.
        if let Ok(ulid) = s.parse::<crate::ulid::Ulid>() {
            // WHY: ULID and UUID are both 128-bit. Reinterpret the ULID's
            // u128 as UUID bytes to produce a stable, round-trippable ID.
            return Ok(Self(Uuid::from_u128(ulid.as_u128())));
        }
        // Legacy `ses_<24hex>` format — pre-ULID aletheia (≤ 0.15). The 24
        // hex chars encode 96 bits; left-pad with 32 zero bits to land in
        // 128 bits and reinterpret as UUID. Deterministic and collision-free
        // within a given DB because the legacy 96-bit space was unique.
        if let Some(rest) = s.strip_prefix("ses_")
            && rest.len() == 24
            && rest.chars().all(|c| c.is_ascii_hexdigit())
            && let Ok(low) = u128::from_str_radix(rest, 16)
        {
            return Ok(Self(Uuid::from_u128(low)));
        }
        Err(IdError::InvalidFormat {
            kind: "SessionId",
            value: s.to_owned(),
            reason: "invalid session ID (expected UUID, ULID, or legacy ses_<24hex> format)"
                .to_owned(),
        })
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<String> for SessionId {
    type Error = IdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.0.to_string()
    }
}

/// A turn identifier. Sequential within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct TurnId(u64);

impl TurnId {
    /// Create a turn ID from a numeric value.
    #[must_use]
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    /// The underlying numeric value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// The next sequential turn ID.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl From<u64> for TurnId {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

impl From<TurnId> for u64 {
    fn from(id: TurnId) -> Self {
        id.0
    }
}

impl fmt::Display for TurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A tool name. Validated to be non-empty and contain only safe characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToolName(String);

impl ToolName {
    /// Construct a `ToolName` from a string literal known to be valid at compile time.
    ///
    /// The caller is responsible for passing a known-valid literal.
    #[must_use]
    pub fn from_static(name: &'static str) -> Self {
        Self(name.to_owned())
    }

    /// Create a new tool name.
    ///
    /// # Errors
    /// Returns an error if the name is empty, exceeds 128 characters,
    /// or contains characters other than alphanumeric, hyphens, and underscores.
    #[must_use = "returns a validated tool name that should not be discarded"]
    pub fn new(name: impl Into<String>) -> Result<Self, IdError> {
        let name = name.into();
        if name.is_empty() {
            return Err(IdError::Empty { kind: "ToolName" });
        }
        if name.len() > 128 {
            return Err(IdError::TooLong {
                kind: "ToolName",
                max: 128,
                actual: name.len(),
            });
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(IdError::InvalidFormat {
                kind: "ToolName",
                value: name.clone(),
                reason: "must contain only alphanumeric, hyphens, and underscores".to_owned(),
            });
        }
        Ok(Self(name))
    }

    /// The underlying string value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ToolName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ToolName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ToolName {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<ToolName> for String {
    fn from(name: ToolName) -> Self {
        name.0
    }
}

fn validate_id(id: &str, kind: &'static str) -> Result<(), IdError> {
    if id.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if id.len() > MAX_NOUS_ID_LEN {
        return Err(IdError::TooLong {
            kind,
            max: MAX_NOUS_ID_LEN,
            actual: id.len(),
        });
    }
    if let Some(prefix) = RESERVED_NOUS_ID_PREFIXES
        .iter()
        .find(|prefix| id.starts_with(**prefix))
    {
        return Err(IdError::InvalidFormat {
            kind,
            value: id.to_owned(),
            reason: format!("must not use reserved internal prefix {prefix:?}"),
        });
    }
    if id.contains('\0') {
        return Err(IdError::InvalidFormat {
            kind,
            value: id.to_owned(),
            reason: "must not contain NUL bytes".to_owned(),
        });
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(IdError::InvalidFormat {
            kind,
            value: id.to_owned(),
            reason: "must contain only lowercase alphanumeric and hyphens".to_owned(),
        });
    }
    if id.starts_with('-') || id.ends_with('-') {
        return Err(IdError::InvalidFormat {
            kind,
            value: id.to_owned(),
            reason: "must not start or end with a hyphen".to_owned(),
        });
    }
    Ok(())
}

/// Errors from identifier construction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// The identifier was empty.
    Empty {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
    },
    /// The identifier exceeded the maximum length.
    TooLong {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
        /// Maximum allowed length.
        max: usize,
        /// Actual length that was provided.
        actual: usize,
    },
    /// The identifier contained invalid characters or format.
    InvalidFormat {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
        /// The value that failed validation.
        value: String,
        /// Description of why the format is invalid.
        reason: String,
    },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(f, "{kind} cannot be empty"),
            Self::TooLong { kind, max, actual } => {
                write!(f, "{kind} too long: {actual} chars (max {max})")
            }
            Self::InvalidFormat {
                kind,
                value,
                reason,
            } => write!(f, "invalid {kind} '{value}': {reason}"),
        }
    }
}

impl std::error::Error for IdError {}

#[cfg(test)]
#[path = "id_tests.rs"]
mod id_tests;
