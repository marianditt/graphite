use std::collections::HashMap;

use graphite_core::*;

#[test]
fn test_node_serde_roundtrip() {
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    edges.insert("depends_on".to_string(), vec!["node_b".to_string()]);

    let mut metadata: HashMap<String, String> = HashMap::new();
    metadata.insert("source".to_string(), "test".to_string());

    let index = Index {
        of_kind: "function".to_string(),
    };

    let node = Node {
        id: "node_a".to_string(),
        kind: "function".to_string(),
        body: "pub fn hello() {}".to_string(),
        edges,
        metadata,
        index: Some(index),
        content_len: 22,
    };

    let json = serde_json::to_string_pretty(&node).expect("serialize node");
    let deserialized: Node = serde_json::from_str(&json).expect("deserialize node");
    assert_eq!(node, deserialized);
}

#[test]
fn test_diagnostic_serialization() {
    let diagnostic = Diagnostic {
        rule: "missing-return-type".to_string(),
        severity: Severity::Error,
        node_id: Some("fn_123".to_string()),
        file: Some("src/main.rs".to_string()),
        detail: "Function does not declare a return type.".to_string(),
        fix: "Add `-> ReturnType` to the function signature.".to_string(),
        example: None,
        hint: "Use `fn foo() -> i32 { 42 }` instead.".to_string(),
    };

    let json = serde_json::to_string(&diagnostic).expect("serialize diagnostic");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse diagnostic JSON");

    // All 7 non-None fields should be present in the serialized output:
    // rule, severity, node_id, file, detail, fix, hint
    assert_eq!(value["rule"], "missing-return-type");
    assert_eq!(value["severity"], "Error");
    assert_eq!(value["node_id"], "fn_123");
    assert_eq!(value["file"], "src/main.rs");
    assert_eq!(value["detail"], "Function does not declare a return type.");
    assert_eq!(
        value["fix"],
        "Add `-> ReturnType` to the function signature."
    );
    assert_eq!(value["hint"], "Use `fn foo() -> i32 { 42 }` instead.");

    // The optional `example` field is None and should be absent
    assert!(value.get("example").is_none());
}

#[test]
fn test_severity_enum() {
    let error_json = serde_json::to_string(&Severity::Error).expect("serialize Error");
    assert_eq!(error_json, "\"Error\"");

    let warning_json = serde_json::to_string(&Severity::Warning).expect("serialize Warning");
    assert_eq!(warning_json, "\"Warning\"");

    let error_deserialized: Severity =
        serde_json::from_str("\"Error\"").expect("deserialize Error");
    assert_eq!(error_deserialized, Severity::Error);

    let warning_deserialized: Severity =
        serde_json::from_str("\"Warning\"").expect("deserialize Warning");
    assert_eq!(warning_deserialized, Severity::Warning);
}
