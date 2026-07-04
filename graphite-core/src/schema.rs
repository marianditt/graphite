use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::{Diagnostic, EdgeDef, KindDef, Schema, Severity};

const BUILT_IN_KINDS: [&str; 3] = ["any", "index", "evidence"];

// @graphite:evidence schema-flex-req-ev
// @graphite:evidence schema-driven-arc-ev
pub struct SchemaParser;

#[derive(Deserialize)]
struct RawKindDef {
    key: String,
}

#[derive(Deserialize)]
struct RawEdge {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct RawSchema {
    kinds: HashMap<String, RawKindDef>,
    edges: HashMap<String, RawEdge>,
}

impl SchemaParser {
    /// Parse a YAML string into a validated [`Schema`].
    /// Returns a tutored [`Diagnostic`] on any parse or validation failure.
    #[allow(clippy::result_large_err)]
    pub fn parse(yaml: &str) -> Result<Schema, Diagnostic> {
        check_yaml_section_duplicates(yaml, "kinds")?;
        check_yaml_section_duplicates(yaml, "edges")?;

        // @graphite:evidence serde-yaml-ev
        let raw: RawSchema = serde_yaml::from_str(yaml).map_err(|e| Diagnostic {
            rule: "schema-parse-error".to_string(),
            severity: Severity::Error,
            node_id: None,
            file: None,
            detail: format!("Invalid YAML: {}", e),
            fix: "Ensure the schema YAML is valid and all fields are correctly formatted."
                .to_string(),
            example: None,
            hint: "Check the YAML syntax and field names.".to_string(),
        })?;

        let builtin_set: HashSet<&str> = BUILT_IN_KINDS.iter().copied().collect();
        let mut edges = Vec::with_capacity(raw.edges.len());

        for (name, edge) in raw.edges {
            if !is_valid_kind_ref(&edge.from, &raw.kinds, &builtin_set) {
                return Err(make_undefined_kind_error(&name, "from", &edge.from));
            }
            if !is_valid_kind_ref(&edge.to, &raw.kinds, &builtin_set) {
                return Err(make_undefined_kind_error(&name, "to", &edge.to));
            }

            edges.push(EdgeDef {
                name,
                from: edge.from,
                to: edge.to,
            });
        }

        let kinds: HashMap<String, KindDef> = raw
            .kinds
            .into_iter()
            .map(|(name, kd)| (name, KindDef { key: kd.key }))
            .collect();

        Ok(Schema { kinds, edges })
    }

    pub fn default_schema() -> Schema {
        Schema {
            kinds: HashMap::from([
                (
                    "requirement".to_string(),
                    KindDef {
                        key: "REQ".to_string(),
                    },
                ),
                (
                    "adr".to_string(),
                    KindDef {
                        key: "ADR".to_string(),
                    },
                ),
                (
                    "service".to_string(),
                    KindDef {
                        key: "SVC".to_string(),
                    },
                ),
                (
                    "test".to_string(),
                    KindDef {
                        key: "TST".to_string(),
                    },
                ),
                (
                    "compliance".to_string(),
                    KindDef {
                        key: "CPL".to_string(),
                    },
                ),
                (
                    "runbook".to_string(),
                    KindDef {
                        key: "RBK".to_string(),
                    },
                ),
                (
                    "infra".to_string(),
                    KindDef {
                        key: "INF".to_string(),
                    },
                ),
            ]),
            edges: vec![
                EdgeDef {
                    name: "implemented_by".to_string(),
                    from: "requirement".to_string(),
                    to: "service".to_string(),
                },
                EdgeDef {
                    name: "verified_by".to_string(),
                    from: "requirement".to_string(),
                    to: "test".to_string(),
                },
                EdgeDef {
                    name: "describes".to_string(),
                    from: "adr".to_string(),
                    to: "service".to_string(),
                },
                EdgeDef {
                    name: "references".to_string(),
                    from: "any".to_string(),
                    to: "any".to_string(),
                },
                EdgeDef {
                    name: "relates_to".to_string(),
                    from: "any".to_string(),
                    to: "any".to_string(),
                },
                EdgeDef {
                    name: "evidence".to_string(),
                    from: "any".to_string(),
                    to: "evidence".to_string(),
                },
            ],
        }
    }
}

fn is_valid_kind_ref(
    kind: &str,
    kinds: &HashMap<String, RawKindDef>,
    builtin: &HashSet<&str>,
) -> bool {
    kind == "any" || builtin.contains(kind) || kinds.contains_key(kind)
}

fn make_undefined_kind_error(edge_name: &str, field: &str, kind: &str) -> Diagnostic {
    Diagnostic {
        rule: "schema-parse-error".to_string(),
        severity: Severity::Error,
        node_id: None,
        file: None,
        detail: format!(
            "Edge '{}' references undefined kind '{}' in '{}'",
            edge_name, kind, field
        ),
        fix: format!(
            "Define kind '{}' in the 'kinds' section, use 'any' for a wildcard, \
             or use a built-in kind ('index', 'evidence').",
            kind
        ),
        example: Some(format!(
            "  kinds:\n    {}:\n      key: {}\n",
            kind,
            kind.to_uppercase()
        )),
        hint: "All kinds referenced in edges must be defined in the 'kinds' section \
               or be built-in kinds (index, evidence, any)."
            .to_string(),
    }
}

/// Scan raw YAML text for duplicate mapping keys in a top-level section
/// (`kinds` or `edges`). serde_yaml silently deduplicates `HashMap` keys
/// on deserialization, so we inspect the text ourselves.
#[allow(clippy::result_large_err)]
fn check_yaml_section_duplicates(yaml: &str, section: &str) -> Result<(), Diagnostic> {
    let marker = format!("{}:", section);
    let mut in_section = false;
    let mut seen: HashMap<String, usize> = HashMap::new();

    for line in yaml.lines() {
        let trimmed = line.trim();

        if trimmed == marker {
            in_section = true;
            continue;
        }

        if in_section {
            let indent = line.len() - trimmed.len();
            if indent == 0 && trimmed.ends_with(':') && !trimmed.contains(' ') {
                break;
            }

            if indent > 0
                && let Some(colon) = trimmed.find(':')
            {
                let candidate = trimmed[..colon].trim();
                if !candidate.is_empty()
                    && !candidate.starts_with('-')
                    && candidate
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    *seen.entry(candidate.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    if let Some((key, count)) = seen.iter().find(|&(_, &c)| c > 1) {
        return Err(Diagnostic {
            rule: "schema-parse-error".to_string(),
            severity: Severity::Error,
            node_id: None,
            file: None,
            detail: format!(
                "Duplicate {} name '{}' appears {} times",
                section, key, count
            ),
            fix: format!(
                "Remove duplicate '{}' entries from the '{}' section.",
                key, section
            ),
            example: None,
            hint: format!(
                "Each {} must have a unique name. Rename or remove the duplicate.",
                section
            ),
        });
    }

    Ok(())
}

/// The YAML text of the default schema.
pub const DEFAULT_SCHEMA_YAML: &str = "\
kinds:
  requirement: { key: REQ }
  adr: { key: ADR }
  service: { key: SVC }
  test: { key: TST }
  compliance: { key: CPL }
  runbook: { key: RBK }
  infra: { key: INF }
edges:
  implemented_by: { from: requirement, to: service }
  verified_by: { from: requirement, to: test }
  describes: { from: adr, to: service }
  references: { from: any, to: any }
  relates_to: { from: any, to: any }
  evidence: { from: any, to: evidence }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_schema_yaml() {
        let schema = SchemaParser::parse(DEFAULT_SCHEMA_YAML).expect("default schema should parse");
        assert_eq!(schema.kinds.len(), 7, "should have 7 kinds");
        assert_eq!(schema.edges.len(), 6, "should have 6 edges");
    }

    #[test]
    fn default_schema_struct_matches() {
        let schema = SchemaParser::default_schema();
        assert_eq!(schema.kinds.len(), 7);
        assert_eq!(schema.edges.len(), 6);
    }

    #[test]
    fn invalid_undefined_kind_rejected() {
        let yaml = "\
kinds:
  requirement: { key: REQ }
edges:
  bad_edge: { from: requirement, to: nonexistent }
";
        let err = SchemaParser::parse(yaml).expect_err("undefined kind should produce error");
        assert_eq!(err.rule, "schema-parse-error");
        assert!(
            err.detail.contains("nonexistent"),
            "detail should mention the missing kind"
        );
        assert!(
            err.detail.contains("bad_edge"),
            "detail should mention the edge name"
        );
    }

    #[test]
    fn duplicate_kind_rejected() {
        let yaml = "\
kinds:
  requirement: { key: REQ }
  requirement: { key: REQ2 }
edges:
  test_edge: { from: requirement, to: requirement }
";
        let err = SchemaParser::parse(yaml).expect_err("duplicate kind should produce error");
        assert_eq!(err.rule, "schema-parse-error");
        assert!(
            err.detail.contains("Duplicate"),
            "detail should mention duplicate"
        );
        assert!(
            err.detail.contains("requirement"),
            "detail should name the duplicate key"
        );
    }

    #[test]
    fn duplicate_edge_rejected() {
        let yaml = "\
kinds:
  a: { key: A }
  b: { key: B }
edges:
  same_edge: { from: a, to: b }
  same_edge: { from: b, to: a }
";
        let err = SchemaParser::parse(yaml).expect_err("duplicate edge should produce error");
        assert_eq!(err.rule, "schema-parse-error");
        assert!(
            err.detail.contains("Duplicate"),
            "detail should mention duplicate"
        );
    }

    #[test]
    fn builtin_kinds_allowed() {
        let yaml = "\
kinds:
  requirement: { key: REQ }
edges:
  test_edge: { from: requirement, to: evidence }
  idx_edge: { from: index, to: requirement }
  wild_edge: { from: any, to: any }
";
        let schema = SchemaParser::parse(yaml)
            .expect("built-in kinds (evidence, index, any) should be valid");
        assert_eq!(schema.kinds.len(), 1);
        assert_eq!(schema.edges.len(), 3);
    }

    #[test]
    fn contains_edge_not_in_schema_is_builtin() {
        let yaml = "\
kinds:
  index: { key: IDX }
  service: { key: SVC }
edges:
  contains: { from: index, to: service }
";
        let schema =
            SchemaParser::parse(yaml).expect("edge named 'contains' with valid kinds should parse");
        assert!(schema.edges.iter().any(|e| e.name == "contains"));
    }

    #[test]
    fn default_edges_have_expected_names() {
        let schema = SchemaParser::default_schema();
        let names: Vec<&str> = schema.edges.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"implemented_by"),
            "expected implemented_by in edges"
        );
        assert!(
            names.contains(&"verified_by"),
            "expected verified_by in edges"
        );
        assert!(names.contains(&"describes"), "expected describes in edges");
        assert!(
            names.contains(&"references"),
            "expected references in edges"
        );
    }
}
