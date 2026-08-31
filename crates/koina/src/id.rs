//! Newtype wrappers for domain identifiers.

use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::uuid::Uuid;

/// Default maximum length, in bytes, accepted by a `newtype_id!`-generated
/// `new()`.
///
/// WHY a single generic ceiling rather than a per-type bound: the macro
/// wraps opaque identifiers from many sources (dispatch ULIDs, tool-call
/// ids, stream request ids) with no shared charset. 256 bytes comfortably
/// covers every real identifier in this codebase while still bounding
/// worst-case allocation from untrusted input. A type that needs a tighter,
/// domain-specific bound (charset, length) is a candidate for a hand-rolled
/// type like [`NousId`] rather than a macro parameter.
pub const NEWTYPE_ID_MAX_LEN: usize = 256;

/// Validation shared by every `newtype_id!`-generated `new()`.
///
/// Not part of the public API — called only from the macro expansion, which
/// is why it takes the type name as a bare `&'static str` rather than
/// something more structured.
///
/// # Errors
/// Returns an error if `value` is empty, exceeds [`NEWTYPE_ID_MAX_LEN`]
/// bytes, or contains an ASCII control character.
#[doc(hidden)]
pub fn newtype_id_validate(value: &str, kind: &'static str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if value.len() > NEWTYPE_ID_MAX_LEN {
        return Err(IdError::TooLong {
            kind,
            max: NEWTYPE_ID_MAX_LEN,
            actual: value.len(),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(IdError::InvalidFormat {
            kind,
            value: value.to_owned(),
            reason: "must not contain control characters".to_owned(),
        });
    }
    Ok(())
}

/// Generate a newtype ID wrapper around a string-like inner type.
///
/// Produces a transparent serde newtype with standard string-like trait
/// implementations. The inner type must implement `AsRef<str>`,
/// `From<String>`, `From<&str>`, and `Into<String>`.
///
/// `new()` validates by construction: empty, oversized (> [`NEWTYPE_ID_MAX_LEN`]
/// bytes), and control-character input are all rejected. This is a generic,
/// permissive floor shared by every generated type, not a domain-specific
/// charset — a type needing lowercase-alnum-plus-hyphen enforcement (like
/// [`NousId`]) still needs its own hand-rolled validator.
///
/// `Deserialize` and `FromStr` route through `new()` (#7088), so untrusted
/// input arriving via serde (an HTTP body, a persisted record) or `.parse()`
/// (a clap argument) is subject to the same floor as explicit construction;
/// a validation failure surfaces as a serde error or an `IdError`.
/// `From<String>`/`From<&str>` remain the only unchecked conversions, for
/// call sites that already hold a trusted value (a literal, a value already
/// validated upstream). Prefer `new()` at any boundary that accepts
/// caller-controlled input.
///
/// # Generated API
///
/// - **Derives:** `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Serialize`
/// - **Traits:** `Deserialize` (validating, via `new()`), `Display`,
///   `FromStr` (validating, `Err = IdError`), `AsRef<str>`, `Borrow<str>`,
///   `Deref<Target=str>`, `From<String>`, `From<&str>`, `From<T> for String`,
///   `PartialEq<str>`
/// - **Methods:** `new()` (validating, fallible), `into_inner()`, `as_str()`
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
/// let id = WidgetId::new("w-1").expect("valid widget id");
/// assert_eq!(id.as_str(), "w-1");
/// assert_eq!(id.to_string(), "w-1");
/// let back: String = id.into_inner();
/// assert_eq!(back, "w-1");
///
/// assert!(WidgetId::new("").is_err());
/// assert!("".parse::<WidgetId>().is_err());
/// ```
#[macro_export]
macro_rules! newtype_id {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash,
            ::serde::Serialize,
        )]
        #[serde(transparent)]
        $vis struct $name($inner);

        /// WHY hand-written rather than derived: a derived transparent
        /// `Deserialize` constructs the inner value directly, bypassing
        /// `new()`'s validation -- and deserialization is precisely the
        /// path that carries untrusted input (#7088). Delegating to the
        /// inner type's `Deserialize` keeps the wire shape identical to
        /// the previous `#[serde(transparent)]` derive; only invalid
        /// values change behavior (they now error).
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let inner = <$inner as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::new(inner).map_err(<D::Error as ::serde::de::Error>::custom)
            }
        }

        impl $name {
            /// Create a new identifier, validating it by construction.
            ///
            /// # Errors
            /// Returns an error if the value is empty, exceeds
            /// `koina::id::NEWTYPE_ID_MAX_LEN` bytes, or contains a
            /// control character.
            #[must_use = "returns a validated identifier that should not be discarded"]
            $vis fn new(id: impl Into<$inner>) -> ::std::result::Result<Self, $crate::id::IdError> {
                let inner: $inner = id.into();
                $crate::id::newtype_id_validate(inner.as_ref(), stringify!($name))?;
                Ok(Self(inner))
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
            type Err = $crate::id::IdError;

            /// WHY fallible: an `Infallible` `FromStr` accepts anything,
            /// making a parsed id weaker than a constructed one (#7088).
            /// Routing through `new()` gives every text entrypoint
            /// (`.parse()`, clap) the same validation floor.
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Self::new(s)
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

/// A nous (agent) identifier. Lowercase alphanumeric + hyphens, 1-64 chars.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NousId(String);

impl NousId {
    /// Create a new `NousId`, validating the format.
    ///
    /// # Errors
    /// Returns an error if the ID is empty, exceeds 64 characters,
    /// or contains characters other than lowercase alphanumeric and hyphens.
    #[must_use = "returns a validated identifier that should not be discarded"]
    pub fn new(id: impl Into<String>) -> Result<Self, IdError> {
        let id = id.into();
        validate_id(&id, "NousId")?;
        Ok(Self(id))
    }

    /// Construct a `NousId` from a string literal known to be valid at compile time.
    ///
    /// The caller is responsible for passing a known-valid literal (mirrors
    /// [`ToolName::from_static`]). Intended for inert placeholders, such as a
    /// `Default` impl's unread base value, that never take a live agent id.
    #[must_use]
    pub fn from_static(id: &'static str) -> Self {
        Self(id.to_owned())
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

/// WHY a hand-written impl: parsing a `NousId` must run the same validation `new` does -- with
/// this type's stricter charset, not the generic `newtype_id!` floor -- or a parsed id is weaker
/// than a constructed one.
///
/// WHY it matters beyond tidiness: with `FromStr`, clap parses CLI arguments straight into a
/// validated `NousId`, so the command-line surface gets the same check #4638 wired into config
/// load — an id with uppercase, an underscore, a leading hyphen or a path separator is rejected
/// at argument-parse time instead of reaching the runtime unchecked.
impl FromStr for NousId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
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
    if id.len() > 64 {
        return Err(IdError::TooLong {
            kind,
            max: 64,
            actual: id.len(),
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
    // WHY(#4638): a leading/trailing hyphen produces an ugly-but-legal
    // directory/route segment (`-agent`, `agent-`) and collides more easily
    // under truncation or concatenation than an interior hyphen. Reject at
    // the one shared validator so every entrypoint (CLI, import, HTTP) gets
    // this for free instead of re-deriving it ad hoc.
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
