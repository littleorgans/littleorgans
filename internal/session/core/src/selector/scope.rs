use crate::namespace::Namespace;
use crate::{SmError, SmResult};

use super::Selector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceScope {
    Default,
    Explicit,
}

enum NamespaceMatch<'a> {
    None,
    Matches,
    Conflict(&'a Namespace),
}

impl Selector {
    pub fn scoped_to_namespace(
        selector: Option<Self>,
        namespace: Namespace,
        scope: NamespaceScope,
    ) -> SmResult<Self> {
        let Some(selector) = selector else {
            return Ok(Self::Namespace { namespace });
        };

        match selector.namespace_match(&namespace) {
            NamespaceMatch::None => Ok(match selector {
                Self::All => Self::Namespace { namespace },
                other => Self::And {
                    selectors: vec![Self::Namespace { namespace }, other],
                },
            }),
            NamespaceMatch::Matches => Ok(selector),
            NamespaceMatch::Conflict(_) if scope == NamespaceScope::Default => Ok(selector),
            NamespaceMatch::Conflict(conflict) => Err(SmError::Message(format!(
                "namespace conflict: requested {namespace} but selector specifies namespace:{conflict}"
            ))),
        }
    }

    fn namespace_match(&self, target: &Namespace) -> NamespaceMatch<'_> {
        match self {
            Self::Namespace { namespace } => {
                if namespace == target {
                    NamespaceMatch::Matches
                } else {
                    NamespaceMatch::Conflict(namespace)
                }
            }
            Self::And { selectors } => {
                let mut state = NamespaceMatch::None;
                for selector in selectors {
                    match selector.namespace_match(target) {
                        NamespaceMatch::Conflict(found) => return NamespaceMatch::Conflict(found),
                        NamespaceMatch::Matches => state = NamespaceMatch::Matches,
                        NamespaceMatch::None => {}
                    }
                }
                state
            }
            Self::Id { .. }
            | Self::Label { .. }
            | Self::Dir { .. }
            | Self::Role { .. }
            | Self::All => NamespaceMatch::None,
        }
    }
}
