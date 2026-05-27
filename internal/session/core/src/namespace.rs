use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

pub const DEFAULT_NAMESPACE: &str = "default";
pub const NAMESPACE_MAX_LEN: usize = 63;
pub const RESERVED_NAMESPACE_PREFIX: &str = "sm-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace(String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceRecord {
    pub namespace: Namespace,
    pub created_at: DateTime<Utc>,
}

impl Namespace {
    /// DNS label length is capped at 63 octets. Namespace slugs use ASCII only,
    /// so byte length and character length are equivalent.
    pub fn new(value: impl Into<String>) -> Result<Self, NamespaceError> {
        let value = value.into();
        validate_namespace_slug(&value)?;
        if value.starts_with(RESERVED_NAMESPACE_PREFIX) {
            return Err(NamespaceError::ReservedPrefix {
                prefix: RESERVED_NAMESPACE_PREFIX,
            });
        }
        Ok(Self(value))
    }

    pub fn for_create(value: impl Into<String>) -> Result<Self, NamespaceError> {
        let namespace = Self::new(value)?;
        namespace.ensure_not_default()?;
        Ok(namespace)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn ensure_not_default(&self) -> Result<(), NamespaceError> {
        if self.as_str() == DEFAULT_NAMESPACE {
            return Err(NamespaceError::ReservedName {
                name: DEFAULT_NAMESPACE,
            });
        }
        Ok(())
    }
}

impl Default for Namespace {
    fn default() -> Self {
        Self(DEFAULT_NAMESPACE.to_string())
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Namespace {
    type Err = NamespaceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for Namespace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Namespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamespaceError {
    #[error("namespace is empty")]
    Empty,
    #[error("namespace is too long: {actual} characters, max {max}")]
    TooLong { actual: usize, max: usize },
    #[error("namespace must start with a lowercase ASCII letter")]
    MustStartWithLetter,
    #[error("namespace contains invalid character: {character:?}")]
    InvalidCharacter { character: char },
    #[error("namespace name is reserved: {name}")]
    ReservedName { name: &'static str },
    #[error("namespace prefix is reserved: {prefix}")]
    ReservedPrefix { prefix: &'static str },
}

fn validate_namespace_slug(value: &str) -> Result<(), NamespaceError> {
    if value.is_empty() {
        return Err(NamespaceError::Empty);
    }
    let actual = value.len();
    if actual > NAMESPACE_MAX_LEN {
        return Err(NamespaceError::TooLong {
            actual,
            max: NAMESPACE_MAX_LEN,
        });
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(NamespaceError::Empty);
    };
    if !first.is_ascii_lowercase() {
        return Err(NamespaceError::MustStartWithLetter);
    }
    for character in chars {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
            continue;
        }
        return Err(NamespaceError::InvalidCharacter { character });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};
    use std::str::FromStr;

    use super::*;

    #[test]
    fn namespace_accepts_valid_usable_slugs() {
        let namespace = Namespace::new("alpha-1").or_panic("expected value");

        assert_eq!(namespace.as_str(), "alpha-1");
        assert_eq!(namespace.to_string(), "alpha-1");
        assert_eq!(namespace.clone().into_string(), "alpha-1");
        assert_eq!(
            Namespace::from_str("alpha-1").or_panic("expected value"),
            namespace
        );
    }

    #[test]
    fn namespace_accepts_default_as_usable_value() {
        assert_eq!(
            Namespace::new(DEFAULT_NAMESPACE).or_panic("expected value"),
            Namespace::default()
        );
    }

    #[test]
    fn namespace_for_create_rejects_reserved_default_name() {
        assert_eq!(
            Namespace::for_create(DEFAULT_NAMESPACE).err_or_panic("expected error"),
            NamespaceError::ReservedName {
                name: DEFAULT_NAMESPACE
            }
        );
    }

    #[test]
    fn namespace_rejects_reserved_sm_prefix_for_use_and_create() {
        assert_eq!(
            Namespace::new("sm-system").err_or_panic("expected error"),
            NamespaceError::ReservedPrefix {
                prefix: RESERVED_NAMESPACE_PREFIX
            }
        );
        assert_eq!(
            Namespace::for_create("sm-system").err_or_panic("expected error"),
            NamespaceError::ReservedPrefix {
                prefix: RESERVED_NAMESPACE_PREFIX
            }
        );
    }

    #[test]
    fn namespace_rejects_invalid_shape_cases() {
        let too_long = format!("a{}", "1".repeat(NAMESPACE_MAX_LEN));

        let cases = [
            ("", NamespaceError::Empty),
            ("1alpha", NamespaceError::MustStartWithLetter),
            ("Alpha", NamespaceError::MustStartWithLetter),
            (
                "alpha beta",
                NamespaceError::InvalidCharacter { character: ' ' },
            ),
            (
                too_long.as_str(),
                NamespaceError::TooLong {
                    actual: NAMESPACE_MAX_LEN + 1,
                    max: NAMESPACE_MAX_LEN,
                },
            ),
        ];

        for (value, expected) in cases {
            assert_eq!(
                Namespace::new(value).err_or_panic("expected error"),
                expected
            );
        }
    }

    #[test]
    fn namespace_rejects_uppercase_after_first_character() {
        assert_eq!(
            Namespace::new("alphaBeta").err_or_panic("expected error"),
            NamespaceError::InvalidCharacter { character: 'B' }
        );
    }

    #[test]
    fn namespace_round_trips_as_json_string() {
        let namespace = Namespace::new("alpha-1").or_panic("expected value");

        let json = serde_json::to_string(&namespace).or_panic("expected value");
        let decoded: Namespace = serde_json::from_str(&json).or_panic("expected value");

        assert_eq!(json, "\"alpha-1\"");
        assert_eq!(decoded, namespace);
    }

    #[test]
    fn namespace_deserialization_validates_slug() {
        let error = serde_json::from_str::<Namespace>("\"Alpha\"").err_or_panic("expected error");

        assert!(error.to_string().contains("must start"));
    }
}
