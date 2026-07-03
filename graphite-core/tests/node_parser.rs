use std::collections::HashMap;

use graphite_core::*;

/// Verify a complete valid .node file parses correctly.
#[test]
fn test_valid_node_parses() {
    let source = "---\nid: my-node\nkind: service\nedges:\n  references:\n    - other-node\nmetadata:\n  key: value\n---\n# My Node\n\nBody content here.\n";

    let node = node_parser::NodeParser::parse(source).expect("valid node should parse");

    assert_eq!(node.id, "my-node");
    assert_eq!(node.kind, "service");
    assert_eq!(node.body, "# My Node\n\nBody content here.\n");

    let mut expected_edges: HashMap<String, Vec<String>> = HashMap::new();
    expected_edges.insert("references".to_string(), vec!["other-node".to_string()]);
    assert_eq!(node.edges, expected_edges);

    let mut expected_metadata: HashMap<String, String> = HashMap::new();
    expected_metadata.insert("key".to_string(), "value".to_string());
    assert_eq!(node.metadata, expected_metadata);

    assert!(node.index.is_none(), "knowledge node should not have index");
}

/// A file without YAML frontmatter must be rejected.
#[test]
fn test_no_frontmatter_error() {
    let source = "just plain text\nno frontmatter here\n";

    let err = node_parser::NodeParser::parse(source).expect_err("should fail without frontmatter");
    assert_eq!(err.rule, "node-parse-error");
    assert_eq!(err.severity, Severity::Error);
    assert!(
        err.detail.contains("frontmatter"),
        "error should mention frontmatter: {}",
        err.detail
    );
}

/// A file with YAML frontmatter but missing the `id` field must return an error.
#[test]
fn test_missing_id_error() {
    let source = "---\nkind: index\nmetadata:\n  of_kind: service\n---\n# Body\n";

    let err = node_parser::NodeParser::parse(source).expect_err("should fail when id is missing");
    assert_eq!(err.rule, "node-parse-error");
    assert!(
        err.detail.contains("id"),
        "error should mention missing id: {}",
        err.detail
    );
}

/// A non-index (knowledge) node with a `contains` edge must be rejected.
#[test]
fn test_knowledge_node_contains_rejected() {
    let source =
        "---\nid: my-node\nkind: service\nedges:\n  contains:\n    - other-node\n---\n# Body\n";

    let err = node_parser::NodeParser::parse(source)
        .expect_err("knowledge node with contains edge should be rejected");
    assert_eq!(err.rule, "node-parse-error");
    assert!(
        err.detail.contains("contains"),
        "error should mention contains: {}",
        err.detail
    );
}

/// An index node with valid `of_kind` metadata and `contains` edges must parse.
#[test]
fn test_index_node_parses() {
    let source = "---\nid: my-index\nkind: index\nedges:\n  contains:\n    - child-node\nmetadata:\n  of_kind: service\n---\n# Index Body\n";

    let node = node_parser::NodeParser::parse(source).expect("index node should parse");

    assert_eq!(node.id, "my-index");
    assert_eq!(node.kind, "index");
    assert_eq!(node.body, "# Index Body\n");

    let mut expected_edges: HashMap<String, Vec<String>> = HashMap::new();
    expected_edges.insert("contains".to_string(), vec!["child-node".to_string()]);
    assert_eq!(node.edges, expected_edges);

    let mut expected_metadata: HashMap<String, String> = HashMap::new();
    expected_metadata.insert("of_kind".to_string(), "service".to_string());
    assert_eq!(node.metadata, expected_metadata);

    let idx = node.index.expect("index node should have index set");
    assert_eq!(idx.of_kind, "service");
}
