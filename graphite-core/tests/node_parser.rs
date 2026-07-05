use graphite_core::*;

/// Verify a complete valid .node file parses correctly.
#[test]
fn test_valid_node_parses() {
    let source = "---\nid: my-node\ncategory: service\n---\n# My Node\n\nBody content here.\n";

    let node = node_parser::NodeParser::parse(source).expect("valid node should parse");

    assert_eq!(node.id, "my-node");
    assert_eq!(node.category, "service");
    assert_eq!(node.body, "# My Node\n\nBody content here.\n");
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
    let source = "---\ncategory: spec\n---\n# Body\n";

    let err = node_parser::NodeParser::parse(source).expect_err("should fail when id is missing");
    assert_eq!(err.rule, "node-parse-error");
    assert!(
        err.detail.contains("id"),
        "error should mention missing id: {}",
        err.detail
    );
}

/// A simple node with only id and category should parse successfully.
#[test]
fn test_minimal_node_parses() {
    let source = "---\nid: minimal\ncategory: spec\n---\n# Minimal\n\nSimple body.\n";

    let node = node_parser::NodeParser::parse(source).expect("minimal node should parse");

    assert_eq!(node.id, "minimal");
    assert_eq!(node.category, "spec");
    assert_eq!(node.body, "# Minimal\n\nSimple body.\n");
}
