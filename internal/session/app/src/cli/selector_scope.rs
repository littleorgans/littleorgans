use std::str::FromStr;

use anyhow::{Result, bail};
use clap::Args;
use lilo_session_core::{Namespace, NamespaceScope, Selector};

use crate::cli::namespace_resolver::resolve_namespace_dir;

#[derive(Debug, Clone, Default, Args)]
pub struct NamespaceScopeArgs {
    #[arg(long, help = "Namespace scope for resolving session selectors")]
    pub namespace: Option<Namespace>,
    #[arg(
        short = 'A',
        long = "all-namespaces",
        conflicts_with = "namespace",
        help = "Resolve session selectors across all namespaces"
    )]
    pub all_namespaces: bool,
}

pub fn scoped_selector(raw: Option<&str>, scope: &NamespaceScopeArgs) -> Result<Option<Selector>> {
    let selector = raw.map(Selector::from_str).transpose()?;
    if scope.all_namespaces {
        return Ok(selector);
    }
    let namespace =
        resolve_namespace_dir(std::env::current_dir()?, scope.namespace.clone())?.namespace;
    let scope = if scope.namespace.is_some() {
        NamespaceScope::Explicit
    } else {
        NamespaceScope::Default
    };
    Ok(Some(Selector::scoped_to_namespace(
        selector, namespace, scope,
    )?))
}

pub fn required_scoped_selector(raw: &str, scope: &NamespaceScopeArgs) -> Result<Selector> {
    let Some(selector) = scoped_selector(Some(raw), scope)? else {
        bail!("selector is required");
    };
    Ok(selector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::OrPanic as _;

    #[test]
    fn all_namespaces_leaves_selector_unscoped() {
        let scope = NamespaceScopeArgs {
            namespace: Some(Namespace::new("alpha").or_panic("expected value")),
            all_namespaces: true,
        };
        let selector = scoped_selector(Some("role:engineer"), &scope)
            .or_panic("selector scopes")
            .or_panic("selector exists");
        assert_eq!(
            selector,
            Selector::Role {
                name: "engineer".to_string()
            }
        );
    }
}
