use std::path::PathBuf;
use std::str::FromStr;

use uuid::Uuid;

use crate::namespace::Namespace;
use crate::test_support::{ErrOrPanic as _, OrPanic as _};

use super::{LabelOp, NamespaceScope, Selector};

#[test]
fn selector_parser_covers_closed_grammar() {
    let id = Uuid::now_v7();

    assert_eq!(
        Selector::from_str("all").or_panic("expected value"),
        Selector::All
    );
    assert_eq!(
        Selector::from_str(&format!("id:{id}")).or_panic("expected value"),
        Selector::Id { id }
    );
    assert_eq!(
        Selector::from_str(&id.to_string()).or_panic("expected value"),
        Selector::Id { id }
    );
    assert_eq!(
        Selector::from_str("role:engineer").or_panic("expected value"),
        Selector::Role {
            name: "engineer".to_string()
        }
    );
    assert_eq!(
        Selector::from_str("namespace:alpha").or_panic("expected value"),
        Selector::Namespace {
            namespace: Namespace::new("alpha").or_panic("expected value")
        }
    );
    assert_eq!(
        Selector::from_str("dir:/tmp/project").or_panic("expected value"),
        Selector::Dir {
            path: PathBuf::from("/tmp/project")
        }
    );
    assert_eq!(
        Selector::from_str("label:area=auth").or_panic("expected value"),
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::Eq {
                value: "auth".to_string()
            }
        }
    );
    assert_eq!(
        Selector::from_str("label:area in (auth, ui)").or_panic("expected value"),
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::In {
                values: vec!["auth".to_string(), "ui".to_string()]
            }
        }
    );
}

#[test]
fn selector_display_round_trips_through_from_str() {
    let id = Uuid::now_v7();
    let cases = vec![
        Selector::All,
        Selector::Id { id },
        Selector::Role {
            name: "engineer".to_string(),
        },
        Selector::Namespace {
            namespace: Namespace::new("alpha").or_panic("expected value"),
        },
        Selector::Dir {
            path: PathBuf::from("/tmp/project"),
        },
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::Eq {
                value: "auth".to_string(),
            },
        },
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::In {
                values: vec!["auth".to_string(), "ui".to_string()],
            },
        },
    ];
    for selector in cases {
        let rendered = selector.to_string();
        let parsed = Selector::from_str(&rendered).or_panic("expected value");
        assert_eq!(parsed, selector, "round-trip failed for {rendered}");
    }

    assert_eq!(Selector::All.to_string(), "all");
    assert_eq!(
        Selector::Role {
            name: "engineer".to_string(),
        }
        .to_string(),
        "role:engineer"
    );
    assert_eq!(
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::Eq {
                value: "auth".to_string(),
            },
        }
        .to_string(),
        "label:area=auth"
    );
    assert_eq!(
        Selector::Label {
            key: "area".to_string(),
            op: LabelOp::In {
                values: vec!["auth".to_string(), "ui".to_string()],
            },
        }
        .to_string(),
        "label:area in (auth, ui)"
    );
}

#[test]
fn selector_rejects_legacy_workspace_and_invalid_new_selectors() {
    let workspace = Selector::from_str("workspace:test")
        .err_or_panic("expected error")
        .to_string();
    assert!(workspace.contains("unsupported selector"));
    assert!(workspace.contains("namespace:<slug>"));

    let namespace = Selector::from_str("namespace:SM")
        .err_or_panic("expected error")
        .to_string();
    assert!(namespace.contains("invalid namespace selector"));

    let dir = Selector::from_str("dir:")
        .err_or_panic("expected error")
        .to_string();
    assert_eq!(dir, "dir selector is empty");
}

#[test]
fn namespace_scope_composes_unscoped_selectors() {
    let alpha = Namespace::new("alpha").or_panic("expected value");

    assert_eq!(
        Selector::scoped_to_namespace(None, alpha.clone(), NamespaceScope::Default)
            .or_panic("expected value"),
        Selector::Namespace {
            namespace: alpha.clone()
        }
    );
    assert_eq!(
        Selector::scoped_to_namespace(Some(Selector::All), alpha.clone(), NamespaceScope::Default,)
            .or_panic("expected value"),
        Selector::Namespace {
            namespace: alpha.clone()
        }
    );
    assert_eq!(
        Selector::scoped_to_namespace(
            Some(Selector::Role {
                name: "engineer".to_string()
            }),
            alpha.clone(),
            NamespaceScope::Default,
        )
        .or_panic("expected value"),
        Selector::And {
            selectors: vec![
                Selector::Namespace { namespace: alpha },
                Selector::Role {
                    name: "engineer".to_string()
                }
            ]
        }
    );
}

#[test]
fn namespace_scope_preserves_namespace_selectors_at_any_depth() {
    let alpha = Namespace::new("alpha").or_panic("expected value");
    let beta = Namespace::new("beta").or_panic("expected value");

    let bare = Selector::Namespace {
        namespace: beta.clone(),
    };
    assert_eq!(
        Selector::scoped_to_namespace(Some(bare.clone()), alpha.clone(), NamespaceScope::Default,)
            .or_panic("expected value"),
        bare
    );

    let role_then_namespace = Selector::And {
        selectors: vec![
            Selector::Role {
                name: "engineer".to_string(),
            },
            Selector::Namespace {
                namespace: beta.clone(),
            },
        ],
    };
    assert_eq!(
        Selector::scoped_to_namespace(
            Some(role_then_namespace.clone()),
            alpha.clone(),
            NamespaceScope::Default,
        )
        .or_panic("expected value"),
        role_then_namespace
    );

    let namespace_then_role = Selector::And {
        selectors: vec![
            Selector::Namespace {
                namespace: beta.clone(),
            },
            Selector::Role {
                name: "engineer".to_string(),
            },
        ],
    };
    assert_eq!(
        Selector::scoped_to_namespace(
            Some(namespace_then_role.clone()),
            alpha.clone(),
            NamespaceScope::Default,
        )
        .or_panic("expected value"),
        namespace_then_role
    );

    let nested = Selector::And {
        selectors: vec![Selector::And {
            selectors: vec![Selector::Namespace { namespace: beta }],
        }],
    };
    assert_eq!(
        Selector::scoped_to_namespace(Some(nested.clone()), alpha, NamespaceScope::Default)
            .or_panic("expected value"),
        nested
    );
}

#[test]
fn explicit_namespace_scope_accepts_matching_namespace_selector() {
    let alpha = Namespace::new("alpha").or_panic("expected value");
    let selector = Selector::And {
        selectors: vec![
            Selector::Namespace {
                namespace: alpha.clone(),
            },
            Selector::Role {
                name: "engineer".to_string(),
            },
        ],
    };

    assert_eq!(
        Selector::scoped_to_namespace(Some(selector.clone()), alpha, NamespaceScope::Explicit,)
            .or_panic("expected value"),
        selector
    );
}

#[test]
fn explicit_namespace_scope_rejects_mismatched_namespace_selector() {
    let alpha = Namespace::new("alpha").or_panic("expected value");
    let beta = Namespace::new("beta").or_panic("expected value");
    let error = Selector::scoped_to_namespace(
        Some(Selector::Namespace { namespace: beta }),
        alpha,
        NamespaceScope::Explicit,
    )
    .err_or_panic("expected error")
    .to_string();

    assert!(error.contains("namespace conflict"));
    assert!(error.contains("alpha"));
    assert!(error.contains("namespace:beta"));
}
