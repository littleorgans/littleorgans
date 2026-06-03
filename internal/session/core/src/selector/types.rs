use std::path::PathBuf;

use lilo_common::id::SessionId;
use serde::{Deserialize, Serialize};

use crate::namespace::Namespace;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum Selector {
    Id {
        id: SessionId,
    },
    Label {
        key: String,
        op: LabelOp,
    },
    Namespace {
        namespace: Namespace,
    },
    Dir {
        path: PathBuf,
    },
    And {
        selectors: Vec<Selector>,
    },
    Role {
        name: String,
    },
    #[default]
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LabelOp {
    Eq { value: String },
    In { values: Vec<String> },
}
