use std::collections::HashMap;

use serde::Deserialize;

use crate::{Diagnostic, Index, Node, Severity};

/// Deserialization-only struct matching the YAML frontmatter shape.
// @graphite:evidence spec-header
#[derive(Deserialize)]
struct Frontmatter {
    id: Option<String>,
    kind: Option<String>,
    edges: Option<HashMap<String, Vec<String>>>,
    metadata: Option<HashMap<String, String>>,
}

// @graphite:evidence spec-document-format
// @graphite:evidence spec-header
// @graphite:evidence spec-body
// @graphite:evidence spec-markdown-extension
// @graphite:evidence spec-index-node
/// Parses `.node` files (YAML frontmatter + Markdown body) into [`Node`] structs.
///
/// # Errors
///
/// Returns a tutoring [`Diagnostic`] with `rule = "node-parse-error"` for any
/// parse failure (missing frontmatter, invalid YAML, missing required fields,
/// or invalid edge constraints).
pub struct NodeParser;

impl NodeParser {
    /// Parse a `.node` file from its raw string content.
    ///
    /// The file must begin with `---`, followed by YAML frontmatter, a closing
    /// `---`, then the Markdown body.
    #[allow(clippy::result_large_err)]
    pub fn parse(source: &str) -> Result<Node, Diagnostic> {
        // @graphite:evidence spec-header
        // @graphite:evidence spec-body
        if !source.starts_with("---") {
            return Err(Self::no_frontmatter_diagnostic());
        }

        let source_bytes = source.as_bytes();
        let len = source.len();

        // Position immediately after the opening "---" (skip past optional '\n')
        let after_opening = if len > 3 && source_bytes[3] == b'\n' {
            4
        } else {
            3
        };

        let remaining = &source[after_opening..];

        // Find the closing "---" marker (either at the start of `remaining` or
        // preceded by '\n').
        let closing_rel = if remaining.starts_with("---") {
            Some(0)
        } else {
            remaining.find("\n---").map(|pos| pos + 1)
        };

        let closing_rel = closing_rel.ok_or_else(Self::missing_closing_diagnostic)?;

        // YAML frontmatter lives between the two "---" markers.
        let yaml_str = &source[after_opening..after_opening + closing_rel];

        // Body starts after the closing "---".
        let body_start = after_opening + closing_rel + 3;
        let body = if body_start < len {
            if source_bytes[body_start] == b'\n' {
                source[body_start + 1..].to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // -- YAML parsing -----------------------------------------------------------
        let frontmatter = Self::parse_frontmatter(yaml_str)?;

        // -- required-field validation ----------------------------------------------
        let id = frontmatter.id.ok_or_else(|| {
            Self::missing_field_diagnostic(
                "id",
                "Every .node file must declare an `id` field in its frontmatter. \
                 For example:\n\n---\nid: my-unique-node\nkind: service\n---",
            )
        })?;

        let kind = frontmatter.kind.ok_or_else(|| {
            Self::missing_field_diagnostic(
                "kind",
                "Every .node file must declare a `kind` field in its frontmatter. \
                 For example:\n\n---\nid: my-node\nkind: service\n---",
            )
        })?;

        let edges = frontmatter.edges.unwrap_or_default();
        let metadata = frontmatter.metadata.unwrap_or_default();

        // -- index-specific validation ----------------------------------------------
        let index = if kind == "index" {
            let of_kind = metadata
                .get("of_kind")
                .ok_or_else(Self::missing_of_kind_diagnostic)?;

            // Ensure "contains" is the only allowed edge type.
            for edge_kind in edges.keys() {
                if edge_kind != "contains" {
                    return Err(Self::forbidden_edge_diagnostic(
                        edge_kind,
                        "index",
                        "Index nodes must use only `contains` edges. \
                         For example:\n\n---\nid: my-index\nkind: index\n\
                         edges:\n  contains:\n    - child-node\n\
                         metadata:\n  of_kind: service\n---",
                    ));
                }
            }

            Some(Index {
                of_kind: of_kind.clone(),
            })
        } else {
            // Knowledge nodes must NOT have a "contains" edge.
            if edges.contains_key("contains") {
                return Err(Self::forbidden_edge_diagnostic(
                    "contains",
                    "knowledge",
                    "The `contains` edge is reserved for index nodes. Remove it from this node. \
                     For example:\n\n---\nid: my-node\nkind: service\n\
                     edges:\n  references:\n    - other-node\n---",
                ));
            }
            None
        };

        Ok(Node {
            id,
            kind,
            body,
            edges,
            metadata,
            index,
            content_len: source.len(),
        })
    }

    // ---------------------------------------------------------------------------
    // Helper: YAML deserialization
    // ---------------------------------------------------------------------------

    #[allow(clippy::result_large_err)]
    fn parse_frontmatter(yaml_str: &str) -> Result<Frontmatter, Diagnostic> {
        match serde_yaml::from_str::<Frontmatter>(yaml_str) {
            Ok(f) => Ok(f),
            Err(_) if yaml_str.trim().is_empty() => Ok(Frontmatter {
                id: None,
                kind: None,
                edges: None,
                metadata: None,
            }),
            Err(e) => Err(Self::parse_error_diagnostic(&e.to_string())),
        }
    }

    // ---------------------------------------------------------------------------
    // Diagnostic builders
    // ---------------------------------------------------------------------------

    fn diagnostic(detail: &str, fix: &str, hint: &str) -> Diagnostic {
        Diagnostic {
            rule: "node-parse-error".to_string(),
            severity: Severity::Error,
            node_id: None,
            file: None,
            detail: detail.to_string(),
            fix: fix.to_string(),
            example: None,
            hint: hint.to_string(),
        }
    }

    fn no_frontmatter_diagnostic() -> Diagnostic {
        Self::diagnostic(
            "File does not contain YAML frontmatter. A valid .node file must start with `---` on the first line.",
            "Add `---` as the first line, followed by YAML frontmatter, then a closing `---`.",
            "A .node file has two sections separated by `---` delimiters. \
             Example:\n\n---\nid: my-node\nkind: service\n---\n\n# Body content",
        )
    }

    fn missing_closing_diagnostic() -> Diagnostic {
        Self::diagnostic(
            "Frontmatter is not properly closed. A valid .node file must have a closing `---` delimiter after the YAML frontmatter.",
            "Add a closing `---` line after the YAML frontmatter and before the body content.",
            "The frontmatter section must be delimited by `---` on both sides:\n\n\
             ---\nid: my-node\nkind: service\n---\n\n# Body content",
        )
    }

    fn parse_error_diagnostic(err: &str) -> Diagnostic {
        Self::diagnostic(
            &format!("Failed to parse YAML frontmatter: {err}"),
            "Fix the syntax error in the YAML frontmatter between the `---` delimiters.",
            "YAML frontmatter must be valid YAML. Ensure proper indentation and quoting. \
             Example:\n\n---\nid: my-node\nkind: service\nmetadata:\n  key: value\n---",
        )
    }

    fn missing_field_diagnostic(field: &str, hint: &str) -> Diagnostic {
        Self::diagnostic(
            &format!("Required field `{field}` is missing from the YAML frontmatter."),
            &format!("Add `{field}: <value>` to the frontmatter."),
            hint,
        )
    }

    fn missing_of_kind_diagnostic() -> Diagnostic {
        Self::diagnostic(
            "Index node is missing `of_kind` in metadata. Index nodes require `of_kind` to specify what kind of nodes they index.",
            "Add `of_kind` to the metadata section, for example:\n\n\
             ---\nid: my-index\nkind: index\nmetadata:\n  of_kind: service\n---",
            "Index nodes must declare `of_kind` in their metadata so the graph knows which node kind this index targets.",
        )
    }

    fn forbidden_edge_diagnostic(edge_kind: &str, node_type: &str, hint: &str) -> Diagnostic {
        Self::diagnostic(
            &format!("Edge type `{edge_kind}` is not allowed on {node_type} nodes."),
            &format!("Remove the `{edge_kind}` edge from this {node_type} node."),
            hint,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_knowledge_node_parses() {
        let source = "\
---\n\
id: my-node\n\
kind: service\n\
edges:\n  references:\n    - other-node\n\
metadata:\n  key: value\n\
---\n\
# My Node\n\n\
Body content here.\n";

        let node = NodeParser::parse(source).expect("valid node should parse");

        assert_eq!(node.id, "my-node");
        assert_eq!(node.kind, "service");
        assert_eq!(node.body, "# My Node\n\nBody content here.\n");

        let mut expected_edges: HashMap<String, Vec<String>> = HashMap::new();
        expected_edges.insert("references".to_string(), vec!["other-node".to_string()]);
        assert_eq!(node.edges, expected_edges);

        let mut expected_metadata: HashMap<String, String> = HashMap::new();
        expected_metadata.insert("key".to_string(), "value".to_string());
        assert_eq!(node.metadata, expected_metadata);

        assert!(node.index.is_none());
    }

    #[test]
    fn test_no_frontmatter_error() {
        let source = "just plain text\nno frontmatter here";

        let err = NodeParser::parse(source).expect_err("should fail without frontmatter");
        assert_eq!(err.rule, "node-parse-error");
        assert_eq!(err.severity, Severity::Error);
        assert!(
            err.detail.contains("frontmatter"),
            "error should mention frontmatter: {}",
            err.detail
        );
    }

    #[test]
    fn test_missing_id_error() {
        let source = "---\nkind: service\n---\n# Body";

        let err = NodeParser::parse(source).expect_err("should fail when id is missing");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("id"),
            "error should mention id: {}",
            err.detail
        );
    }

    #[test]
    fn test_missing_kind_error() {
        let source = "---\nid: my-node\n---\n# Body";

        let err = NodeParser::parse(source).expect_err("should fail when kind is missing");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("kind"),
            "error should mention kind: {}",
            err.detail
        );
    }

    #[test]
    fn test_knowledge_node_contains_rejected() {
        let source =
            "---\nid: my-node\nkind: service\nedges:\n  contains:\n    - other-node\n---\n# Body";

        let err = NodeParser::parse(source)
            .expect_err("knowledge node with contains edge should be rejected");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("contains"),
            "error should mention contains: {}",
            err.detail
        );
    }

    #[test]
    fn test_index_node_parses() {
        let source = "\
---\n\
id: my-index\n\
kind: index\n\
edges:\n  contains:\n    - child-node\n\
metadata:\n  of_kind: service\n\
---\n\
# Index Body\n";

        let node = NodeParser::parse(source).expect("index node should parse");

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

    #[test]
    fn test_index_node_without_of_kind_rejected() {
        let source =
            "---\nid: my-index\nkind: index\nedges:\n  contains:\n    - child-node\n---\n# Body";

        let err =
            NodeParser::parse(source).expect_err("index node without of_kind should be rejected");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("of_kind"),
            "error should mention of_kind: {}",
            err.detail
        );
    }

    #[test]
    fn test_missing_closing_delimiter_error() {
        let source = "---\nid: my-node\nkind: service\n";

        let err = NodeParser::parse(source).expect_err("should fail without closing ---");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("closed") || err.detail.contains("closing"),
            "error should mention missing closing delimiter: {}",
            err.detail
        );
    }

    #[test]
    fn test_empty_body_is_allowed() {
        let source = "---\nid: my-node\nkind: service\n---";

        let node = NodeParser::parse(source).expect("node with empty body should parse");
        assert_eq!(node.id, "my-node");
        assert_eq!(node.kind, "service");
        assert_eq!(node.body, "");
    }

    #[test]
    fn test_edges_optional() {
        let source = "---\nid: my-node\nkind: service\n---\n# Body";

        let node = NodeParser::parse(source).expect("node without edges should parse");
        assert_eq!(node.id, "my-node");
        assert!(node.edges.is_empty());
    }

    #[test]
    fn test_metadata_optional() {
        let source = "---\nid: my-node\nkind: service\n---\n# Body";

        let node = NodeParser::parse(source).expect("node without metadata should parse");
        assert_eq!(node.id, "my-node");
        assert!(node.metadata.is_empty());
    }

    #[test]
    fn test_invalid_yaml_error() {
        let source = "---\nid: my-node\nkind: [invalid\n---\n# Body";

        let err = NodeParser::parse(source).expect_err("invalid YAML should fail");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("YAML"),
            "error should mention YAML: {}",
            err.detail
        );
    }
}
