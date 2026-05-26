use lilo_im_core::Principal;
use serde_json::json;

#[test]
fn unknown_principal_kind_deserializes_without_error() {
    let original = json!({
        "kind": "Future",
        "data": {
            "subject": "service-account",
            "namespace": "default"
        }
    });

    let principal: Principal = serde_json::from_value(original.clone()).unwrap();

    assert_eq!(
        principal,
        Principal::Unknown {
            kind: "Future".to_owned(),
            raw: json!({
                "data": {
                    "subject": "service-account",
                    "namespace": "default"
                }
            }),
        }
    );
    assert_eq!(serde_json::to_value(principal).unwrap(), original);
}
