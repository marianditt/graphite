use serde::Deserialize;

use crate::{Diagnostic, Node, Severity};

/// Deserialization-only struct matching the YAML frontmatter shape.
// @graphite:evidence spec-header
#[derive(Deserialize)]
struct Frontmatter {
    id: Option<String>,
    category: Option<String>,
}

// @graphite:evidence spec-document-format
// @graphite:evidence spec-header
// @graphite:evidence spec-body
// @graphite:evidence spec-markdown-extension
/// Parses `.node` files (YAML frontmatter + Markdown body) into [`Node`] structs.
///
/// # Errors
///
/// Returns a tutoring [`Diagnostic`] with `rule = "node-parse-error"` for any
/// parse failure (missing frontmatter, invalid YAML, missing required fields).
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
                 For example:\n\n---\nid: my-unique-node\ncategory: service\n---",
            )
        })?;

        let category = frontmatter.category.ok_or_else(|| {
            Self::missing_field_diagnostic(
                "category",
                "Every .node file must declare a `category` field in its frontmatter. \
                 For example:\n\n---\nid: my-node\ncategory: service\n---",
            )
        })?;

        Ok(Node {
            id,
            category,
            body,
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
                category: None,
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
              Example:\n\n---\nid: my-node\ncategory: service\n---\n\n# Body content",
        )
    }

    fn missing_closing_diagnostic() -> Diagnostic {
        Self::diagnostic(
            "Frontmatter is not properly closed. A valid .node file must have a closing `---` delimiter after the YAML frontmatter.",
            "Add a closing `---` line after the YAML frontmatter and before the body content.",
            "The frontmatter section must be delimited by `---` on both sides:\n\n\
              ---\nid: my-node\ncategory: service\n---\n\n# Body content",
        )
    }

    fn parse_error_diagnostic(err: &str) -> Diagnostic {
        Self::diagnostic(
            &format!("Failed to parse YAML frontmatter: {err}"),
            "Fix the syntax error in the YAML frontmatter between the `---` delimiters.",
            "YAML frontmatter must be valid YAML. Ensure proper indentation and quoting. \
              Example:\n\n---\nid: my-node\ncategory: service\nmetadata:\n  key: value\n---",
        )
    }

    fn missing_field_diagnostic(field: &str, hint: &str) -> Diagnostic {
        Self::diagnostic(
            &format!("Required field `{field}` is missing from the YAML frontmatter."),
            &format!("Add `{field}: <value>` to the frontmatter."),
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
category: service\n\
---\n\
# My Node\n\n\
Body content here.\n";

        let node = NodeParser::parse(source).expect("valid node should parse");

        assert_eq!(node.id, "my-node");
        assert_eq!(node.category, "service");
        assert_eq!(node.body, "# My Node\n\nBody content here.\n");
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
        let source = "---\ncategory: service\n---\n# Body";

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

        let err = NodeParser::parse(source).expect_err("should fail when category is missing");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("category"),
            "error should mention category: {}",
            err.detail
        );
    }

    #[test]
    fn test_missing_closing_delimiter_error() {
        let source = "---\nid: my-node\ncategory: service\n";

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
        let source = "---\nid: my-node\ncategory: service\n---";

        let node = NodeParser::parse(source).expect("node with empty body should parse");
        assert_eq!(node.id, "my-node");
        assert_eq!(node.category, "service");
        assert_eq!(node.body, "");
    }

    #[test]
    fn test_invalid_yaml_error() {
        let source = "---\nid: my-node\ncategory: [invalid\n---\n# Body";

        let err = NodeParser::parse(source).expect_err("invalid YAML should fail");
        assert_eq!(err.rule, "node-parse-error");
        assert!(
            err.detail.contains("YAML"),
            "error should mention YAML: {}",
            err.detail
        );
    }
}
