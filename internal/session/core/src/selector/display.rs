use std::fmt;

use super::{LabelOp, Selector};

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Id { id } => write!(f, "id:{id}"),
            Self::Role { name } => write!(f, "role:{name}"),
            Self::Namespace { namespace } => write!(f, "namespace:{namespace}"),
            Self::Dir { path } => write!(f, "dir:{}", path.display()),
            Self::And { selectors } => {
                let rendered = selectors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" && ");
                write!(f, "{rendered}")
            }
            Self::Label {
                key,
                op: LabelOp::Eq { value },
            } => write!(f, "label:{key}={value}"),
            Self::Label {
                key,
                op: LabelOp::In { values },
            } => write!(f, "label:{key} in ({})", values.join(", ")),
        }
    }
}
