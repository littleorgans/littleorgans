use std::path::PathBuf;
use std::str::FromStr;

use uuid::Uuid;

use crate::label::parse_label_token;
use crate::namespace::Namespace;
use crate::{SmError, SmResult};

use super::{LabelOp, Selector};

pub const SELECTOR_GRAMMAR_HINT: &str = "all, <uuid>, id:<uuid>, role:<name>, namespace:<slug>, dir:<path>, label:<key>=<value>, label:<key> in (v1, v2)";

impl FromStr for Selector {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        let value = value.trim();
        if value == "all" {
            return Ok(Self::All);
        }
        if let Ok(id) = Uuid::parse_str(value) {
            return Ok(Self::Id { id });
        }
        if let Some(raw) = value.strip_prefix("id:") {
            return Ok(Self::Id {
                id: Uuid::parse_str(raw.trim())?,
            });
        }
        if let Some(raw) = value.strip_prefix("role:") {
            return Ok(Self::Role {
                name: parse_label_token(raw, "role selector")?,
            });
        }
        if let Some(raw) = value.strip_prefix("workspace:") {
            return Err(SmError::Message(format!(
                "unsupported selector: workspace:{} (expected one of: {SELECTOR_GRAMMAR_HINT})",
                raw.trim()
            )));
        }
        if let Some(raw) = value.strip_prefix("namespace:") {
            let namespace = Namespace::new(raw.trim()).map_err(|error| {
                SmError::Message(format!("invalid namespace selector: {error}"))
            })?;
            return Ok(Self::Namespace { namespace });
        }
        if let Some(raw) = value.strip_prefix("dir:") {
            let path = raw.trim();
            if path.is_empty() {
                return Err(SmError::Message("dir selector is empty".to_string()));
            }
            return Ok(Self::Dir {
                path: PathBuf::from(path),
            });
        }
        if let Some(raw) = value.strip_prefix("label:") {
            return parse_label_selector(raw);
        }
        Err(SmError::Message(format!(
            "unsupported selector: {value} (expected one of: {SELECTOR_GRAMMAR_HINT})"
        )))
    }
}

fn parse_label_selector(value: &str) -> SmResult<Selector> {
    if let Some((key, raw_value)) = value.split_once('=') {
        return Ok(Selector::Label {
            key: parse_label_token(key, "label key")?,
            op: LabelOp::Eq {
                value: parse_label_token(raw_value, "label value")?,
            },
        });
    }
    let (key, raw_values) = value
        .split_once(" in ")
        .ok_or_else(|| SmError::Message(format!("invalid label selector: {value}")))?;
    let values = parse_label_values(raw_values)?;
    Ok(Selector::Label {
        key: parse_label_token(key, "label key")?,
        op: LabelOp::In { values },
    })
}

fn parse_label_values(value: &str) -> SmResult<Vec<String>> {
    let value = value.trim();
    let Some(value) = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(SmError::Message(format!(
            "invalid label value list: {value}"
        )));
    };
    let values = value
        .split(',')
        .map(|item| parse_label_token(item, "label value"))
        .collect::<SmResult<Vec<_>>>()?;
    if values.is_empty() {
        return Err(SmError::Message("label value list is empty".to_string()));
    }
    Ok(values)
}
