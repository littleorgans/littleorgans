use chrono::{DateTime, Utc};

pub fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(DateTime::<Utc>::from)
}

pub fn parse_optional_timestamp(
    value: Option<String>,
) -> Result<Option<DateTime<Utc>>, chrono::ParseError> {
    value
        .map(|timestamp| parse_timestamp(&timestamp))
        .transpose()
}
