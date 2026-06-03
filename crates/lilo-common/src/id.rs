use std::fmt::{self, Display};
use std::str::FromStr;

pub const MIN_SHORT_PREFIX_LEN: usize = 7;

#[must_use]
pub fn shortest_unambiguous_prefix<F>(full_id: &str, mut is_unique: F) -> &str
where
    F: FnMut(&str) -> bool,
{
    let min_len = MIN_SHORT_PREFIX_LEN.min(full_id.len());

    for len in min_len..=full_id.len() {
        if full_id.is_char_boundary(len) && is_unique(&full_id[..len]) {
            return &full_id[..len];
        }
    }

    full_id
}

macro_rules! define_id {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        #[cfg_attr(feature = "sqlx", derive(sqlx::Type), sqlx(transparent))]
        pub struct $name(::uuid::Uuid);

        #[allow(clippy::new_without_default)]
        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(::uuid::Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(uuid: ::uuid::Uuid) -> Self {
                Self(uuid)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> ::uuid::Uuid {
                self.0
            }

            #[must_use]
            pub const fn into_uuid(self) -> ::uuid::Uuid {
                self.0
            }

            #[must_use]
            pub fn short(&self) -> String {
                self.short_with(|_| true)
            }

            #[must_use]
            pub fn short_with<F>(&self, is_unique: F) -> String
            where
                F: FnMut(&str) -> bool,
            {
                let full_id = self.to_string();
                $crate::id::shortest_unambiguous_prefix(&full_id, is_unique).to_owned()
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                Display::fmt(&self.0, formatter)
            }
        }

        impl FromStr for $name {
            type Err = ::uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Self(::uuid::Uuid::parse_str(value)?))
            }
        }
    };
}

define_id!(SessionId);
define_id!(MessageId);
define_id!(EventId);
define_id!(IntentId);
define_id!(NamespaceId);
define_id!(AuditId);

#[cfg(test)]
mod tests {
    use super::{
        AuditId, EventId, IntentId, MessageId, NamespaceId, SessionId, shortest_unambiguous_prefix,
    };
    use uuid::Uuid;

    const FIXED_UUID: &str = "12345678-1234-4234-9234-123456789abc";

    macro_rules! assert_id_contract {
        ($id_type:ident) => {{
            let uuid = fixed_uuid();
            let id = $id_type::from_uuid(uuid);

            assert_eq!(id.as_uuid(), uuid);
            assert_eq!(id.into_uuid(), uuid);
            assert_eq!(id.to_string(), uuid.to_string());

            let parsed: $id_type = id.to_string().parse().expect("parse id");
            assert!(parsed == id);
        }};
    }

    fn fixed_uuid() -> Uuid {
        Uuid::parse_str(FIXED_UUID).expect("fixed uuid")
    }

    #[test]
    fn all_id_types_roundtrip_full_uuid_display_and_parse() {
        assert_id_contract!(SessionId);
        assert_id_contract!(MessageId);
        assert_id_contract!(EventId);
        assert_id_contract!(IntentId);
        assert_id_contract!(NamespaceId);
        assert_id_contract!(AuditId);
    }

    #[test]
    fn new_generates_uuid_v4() {
        let id = SessionId::new();

        assert_eq!(id.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn serde_json_roundtrip_is_bare_uuid_string() {
        let uuid = fixed_uuid();
        let id = SessionId::from_uuid(uuid);

        let encoded = serde_json::to_string(&id).expect("encode id");
        assert_eq!(encoded, format!("\"{uuid}\""));

        let decoded: SessionId = serde_json::from_str(&encoded).expect("decode id");
        assert_eq!(decoded.into_uuid(), uuid);
    }

    #[test]
    fn short_uses_seven_char_floor_for_singleton_context() {
        let id = SessionId::from_uuid(fixed_uuid());

        assert_eq!(id.short(), "1234567");
    }

    #[test]
    fn shortest_prefix_selects_minimum_unambiguous_hyphenated_prefix() {
        let full_id = FIXED_UUID;
        let prefix = shortest_unambiguous_prefix(full_id, |candidate| candidate.len() == 9);

        assert_eq!(prefix, "12345678-");
    }

    #[test]
    fn short_with_uses_the_same_hyphenated_selector() {
        let id = SessionId::from_uuid(fixed_uuid());

        assert_eq!(id.short_with(|candidate| candidate.len() == 9), "12345678-");
    }

    #[cfg(feature = "sqlx")]
    #[tokio::test]
    async fn sqlx_sqlite_insert_select_roundtrip() {
        let id = SessionId::from_uuid(fixed_uuid());
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect sqlite");

        sqlx::query("CREATE TABLE ids (id BLOB NOT NULL)")
            .execute(&pool)
            .await
            .expect("create ids table");

        sqlx::query("INSERT INTO ids (id) VALUES (?)")
            .bind(id)
            .execute(&pool)
            .await
            .expect("insert id");

        let selected: SessionId = sqlx::query_scalar("SELECT id FROM ids")
            .fetch_one(&pool)
            .await
            .expect("select id");

        assert_eq!(selected.into_uuid(), id.into_uuid());
    }
}
