//! Shared identity newtypes for corpora, sources, and generations.

use std::borrow::Borrow;
use std::fmt;

use serde::de;
use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(CorpusId);
string_id!(SourceName);
string_id!(NativeId);
string_id!(OriginUri);
string_id!(Revision);

/// Corpus-unique public handle identity.
///
/// The textual shape is source-defined. The only context-free invariant is
/// that an identity is not empty; corpus-wide uniqueness is enforced when
/// [`crate::FactStore`] merges source generations.
///
/// Other string-backed identities are intentionally not interchangeable:
///
/// ```compile_fail
/// use anneal_core::{HandleId, SourceName};
///
/// fn read_handle(_: &HandleId) {}
///
/// let source = SourceName::from("markdown");
/// read_handle(&source);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct HandleId(String);

impl HandleId {
    pub fn new(value: impl Into<String>) -> Result<Self, HandleIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(HandleIdError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HandleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

impl AsRef<str> for HandleId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for HandleId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for HandleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("handle id is empty")]
pub struct HandleIdError;

/// Monotonic source generation for one `(corpus, source)` pair.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Generation(u64);

impl Generation {
    pub const fn initial() -> Self {
        Self(1)
    }

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_id_rejects_empty_construction_and_deserialization() {
        assert_eq!(HandleId::new(""), Err(HandleIdError));
        assert!(
            serde_json::from_str::<HandleId>(r#""""#)
                .expect_err("empty serialized handle is rejected")
                .to_string()
                .contains("handle id is empty")
        );
    }

    #[test]
    fn handle_id_preserves_its_exact_json_string() {
        let id = HandleId::new("formal/models/Space.md#A").expect("nonempty handle");

        assert_eq!(
            serde_json::to_string(&id).expect("serialize handle"),
            r#""formal/models/Space.md#A""#
        );
        assert_eq!(
            serde_json::from_str::<HandleId>(r#""formal/models/Space.md#A""#)
                .expect("deserialize handle"),
            id
        );
    }
}
