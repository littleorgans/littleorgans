use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{SmError, SmResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Label {
    pub key: String,
    pub value: String,
}

impl FromStr for Label {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        let (key, value) = value
            .split_once('=')
            .ok_or_else(|| SmError::Message(format!("invalid label mutation: {value}")))?;
        Ok(Self {
            key: parse_label_token(key, "label key")?,
            value: parse_label_token(value, "label value")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LabelMutation {
    Set(Label),
    Remove { key: String },
}

impl FromStr for LabelMutation {
    type Err = SmError;

    fn from_str(value: &str) -> SmResult<Self> {
        if let Some(key) = value.strip_suffix('-') {
            return Ok(Self::Remove {
                key: parse_label_token(key, "label key")?,
            });
        }
        Ok(Self::Set(Label::from_str(value)?))
    }
}

pub(crate) fn parse_label_token(value: &str, field: &'static str) -> SmResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SmError::Message(format!("{field} is empty")));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn label_mutation_parser_distinguishes_set_and_remove() {
        assert_eq!(
            LabelMutation::from_str("pri=urgent").or_panic("expected value"),
            LabelMutation::Set(Label {
                key: "pri".to_string(),
                value: "urgent".to_string()
            })
        );
        assert_eq!(
            LabelMutation::from_str("pri-").or_panic("expected value"),
            LabelMutation::Remove {
                key: "pri".to_string()
            }
        );
    }
}
