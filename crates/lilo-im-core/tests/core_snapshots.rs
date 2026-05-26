use lilo_im_core::{Action, AuditDecision, AuthzError, Principal};

#[test]
fn authz_error_display_is_stable() {
    let cases = [
        AuthzError::Unauthorized {
            principal: Principal::Local(501),
            action: Action::Spawn,
            reason: "policy denied".to_owned(),
        },
        AuthzError::UnknownPrincipal,
        AuthzError::Audit {
            message: "sqlite unavailable".to_owned(),
        },
        AuthzError::Internal {
            message: "peer credentials unavailable".to_owned(),
        },
    ];
    let rendered = cases
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!("authz_error_display", rendered);
}

#[test]
fn audit_decision_json_is_stable() {
    insta::assert_json_snapshot!(
        "audit_decision_json",
        [
            AuditDecision::Allow,
            AuditDecision::Deny {
                reason: "non-local uid".to_owned(),
            },
            AuditDecision::Error {
                message: "audit sink failed".to_owned(),
            },
        ]
    );
}
