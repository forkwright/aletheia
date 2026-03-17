//! Declarative macros for reducing boilerplate across Aletheia crates.

/// Generate a string-wrapping newtype ID with standard trait implementations.
///
/// Produces a newtype struct with `#[serde(transparent)]` and the following impls:
/// `Display`, `FromStr`, `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`,
/// `Serialize`, `Deserialize`, `AsRef<str>`, `Borrow<str>`, `Deref` to `str`,
/// `From<String>`, `From<&str>`, `Into<String>`, and `PartialEq<str>`.
///
/// The inner type must implement `From<String>`, `From<&str>`, `Into<String>`,
/// `AsRef<str>`, and `Display`. Both `String` and `compact_str::CompactString`
/// satisfy these bounds.
///
/// # Examples
///
/// ```
/// use aletheia_koina::newtype_id;
/// use serde::{Deserialize, Serialize};
///
/// newtype_id! {
///     /// A request identifier.
///     pub struct RequestId(String);
/// }
///
/// let id = RequestId::new("req-1");
/// assert_eq!(id.as_str(), "req-1");
/// assert_eq!(id.to_string(), "req-1");
///
/// let parsed: RequestId = "req-2".parse().unwrap();
/// assert_eq!(parsed.into_inner(), "req-2");
/// ```
#[macro_export]
macro_rules! newtype_id {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident($inner:ty);
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        $vis struct $name($inner);

        impl $name {
            /// Create a new identifier from any value convertible to the inner type.
            #[must_use]
            pub fn new(value: impl Into<$inner>) -> Self {
                Self(value.into())
            }

            /// The underlying string value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                AsRef::<str>::as_ref(&self.0)
            }

            /// Consume the wrapper and return the inner value.
            #[must_use]
            pub fn into_inner(self) -> $inner {
                self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Ok(Self(<$inner>::from(s)))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                AsRef::<str>::as_ref(&self.0)
            }
        }

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                AsRef::<str>::as_ref(&self.0)
            }
        }

        impl ::std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                AsRef::<str>::as_ref(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(<$inner>::from(s))
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(<$inner>::from(s))
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.0.into()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                AsRef::<str>::as_ref(&self.0) == other
            }
        }
    };
}
