use std::collections::HashMap;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// IAM subject on the v1 wire boundary.
///
/// Principals use a stable `kind` tag. Unknown kinds keep their raw fields so
/// v1 readers can inspect rows written by newer identity-matters producers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    Local(u32),
    Unknown {
        kind: String,
        raw: serde_json::Value,
    },
}

impl Principal {
    #[must_use]
    pub fn local(uid: u32) -> Self {
        Self::Local(uid)
    }
}

impl Serialize for Principal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Local(uid) => {
                let mut state = serializer.serialize_struct("Principal", 2)?;
                state.serialize_field("kind", "Local")?;
                state.serialize_field("uid", uid)?;
                state.end()
            }
            Self::Unknown { kind, raw } => {
                if let Some(fields) = raw.as_object() {
                    let mut state = serializer.serialize_map(Some(fields.len() + 1))?;
                    state.serialize_entry("kind", kind)?;
                    for (key, value) in fields {
                        if key != "kind" {
                            state.serialize_entry(key, value)?;
                        }
                    }
                    state.end()
                } else {
                    let mut state = serializer.serialize_struct("Principal", 2)?;
                    state.serialize_field("kind", kind)?;
                    state.serialize_field("raw", raw)?;
                    state.end()
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for Principal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PrincipalVisitor)
    }
}

struct PrincipalVisitor;

impl<'de> Visitor<'de> for PrincipalVisitor {
    type Value = Principal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an identity principal object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut kind = None;
        let mut uid = None;
        let mut raw = serde_json::Map::new();

        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value::<serde_json::Value>()?;
            match key.as_str() {
                "kind" => {
                    kind = Some(
                        value
                            .as_str()
                            .ok_or_else(|| de::Error::custom("principal kind must be a string"))?
                            .to_owned(),
                    );
                }
                "uid" => {
                    let parsed = value
                        .as_u64()
                        .ok_or_else(|| de::Error::custom("local principal uid must be a u32"))?;
                    uid = Some(u32::try_from(parsed).map_err(de::Error::custom)?);
                    raw.insert(key, value);
                }
                _ => {
                    raw.insert(key, value);
                }
            }
        }

        match kind.as_deref() {
            Some("Local") => Ok(Principal::Local(
                uid.ok_or_else(|| de::Error::missing_field("uid"))?,
            )),
            Some(kind) => Ok(Principal::Unknown {
                kind: kind.to_owned(),
                raw: serde_json::Value::Object(raw),
            }),
            None => Err(de::Error::missing_field("kind")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Spawn,
    Kill,
    List,
    Read,
    Logs,
    MailSend,
    MailRead,
    Nudge,
    Link,
    Doctor,
    Daemon,
}

impl Action {
    pub const ALL: [Self; 11] = [
        Self::Spawn,
        Self::Kill,
        Self::List,
        Self::Read,
        Self::Logs,
        Self::MailSend,
        Self::MailRead,
        Self::Nudge,
        Self::Link,
        Self::Doctor,
        Self::Daemon,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Claude,
    Codex,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceSpec {
    pub workspace: Option<String>,
    pub role: Option<String>,
    pub runtime: Option<RuntimeKind>,
    pub session_id: Option<Uuid>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authorized {
    pub principal: Principal,
    pub role: String,
    pub capabilities: Vec<Capability>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Principal;

    #[test]
    fn serializes_local_principal_with_stable_kind_tag() {
        let value = serde_json::to_value(Principal::Local(501)).unwrap();

        assert_eq!(value, json!({ "kind": "Local", "uid": 501 }));
    }
}
