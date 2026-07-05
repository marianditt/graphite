use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::{Diagnostic, Graph, Severity};

// @graphite:evidence spec-validate
/// A map from evidence ID to resolved file locations.
///
/// Produced by merging results from `AnchorScanner` and `SidecarResolver`.
pub type ResolvedEvidence = HashMap<String, Vec<(PathBuf, usize)>>;

// @graphite:evidence spec-validate
/// Validates a [`Graph`] against structural constraints.
///
/// Checks run in order:
/// 1. **edge references** — every `[edge:X]` in a node body targets an existing node
/// 2. **evidence references** — every `[evidence:X]` in a node body has a matching anchor
/// 3. **node title** — every node body has at least one heading
/// 4. **no empty body?** — no (already optional)
///
/// Additional anchor validation is available via [`check_evidence_anchors`],
/// to be called after the main validation passes.
pub struct ValidationEngine;

// ---------------------------------------------------------------------------
// Diagnostic helpers
// ---------------------------------------------------------------------------

#[inline]
fn diag_err(rule: &str, node_id: &str, detail: &str, fix: &str, hint: &str) -> Diagnostic {
    Diagnostic {
        rule: rule.to_string(),
        severity: Severity::Error,
        node_id: Some(node_id.to_string()),
        file: None,
        detail: detail.to_string(),
        fix: fix.to_string(),
        example: None,
        hint: hint.to_string(),
    }
}

impl ValidationEngine {
    pub fn validate(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = vec![];
        diagnostics.extend(self.check_edge_refs_exist(graph));
        diagnostics.extend(self.check_node_body_has_title(graph));
        diagnostics
    }

    /// Check that every `[edge:X]` reference in a node body targets an existing node ID.
    fn check_edge_refs_exist(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            for target in extract_edge_refs(&node.body) {
                if !graph.nodes.contains_key(target.as_str()) {
                    diagnostics.push(diag_err(
                        "broken-edge-target",
                        &node.id,
                        &format!(
                            "Node '{}' has [edge:{}] in its body, but no node with ID \
                             '{}' exists in the graph.",
                            node.id, target, target
                        ),
                        &format!(
                            "Either create a new node with id '{}' (in the appropriate \
                             category directory), or correct the reference in node '{}'.",
                            target, node.id
                        ),
                        "Every [edge:X] reference MUST resolve to an existing node \
                         ID in the graph.",
                    ));
                }
            }
        }

        diagnostics
    }

    /// Check that every node body has at least one markdown heading (# title).
    /// Rule: "node-body-title"
    fn check_node_body_has_title(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            let has_heading = node.body.lines().any(|l| l.trim().starts_with("# "));

            if !has_heading {
                diagnostics.push(Diagnostic {
                    rule: "node-body-title".to_string(),
                    severity: Severity::Warning,
                    node_id: Some(node.id.clone()),
                    file: None,
                    detail: format!(
                        "Node '{}' has no top-level heading (# Title) in its body.",
                        node.id
                    ),
                    fix: format!(
                        "Add a level-1 heading to the body of '{}', for example:\n\n\
                         # {}",
                        node.id, node.id
                    ),
                    example: None,
                    hint:
                        "Every node should have a descriptive title as its first heading."
                            .to_string(),
                });
            }
        }

        diagnostics
    }

    /// Check that every resolved evidence anchor is referenced by at least
    /// one node's `[evidence:X]` body reference. Unused anchors indicate orphan
    /// annotations that should either be wired to a node or removed.
    ///
    /// Rule: `"unused-evidence-anchor"`
    pub fn check_unused_anchors(
        &self,
        graph: &Graph,
        resolved: &ResolvedEvidence,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let mut used: HashSet<String> = HashSet::new();
        for node in graph.nodes.values() {
            for id in extract_evidence_refs(&node.body) {
                used.insert(id.to_string());
            }
        }

        for ev_id in resolved.keys() {
            if !used.contains(ev_id.as_str()) {
                diagnostics.push(Diagnostic {
                    rule: "unused-evidence-anchor".to_string(),
                    severity: Severity::Error,
                    node_id: None,
                    file: None,
                    detail: format!(
                        "Evidence anchor '@graphite:evidence {ev_id}' has no corresponding \
                         '[evidence:{ev_id}]' reference in any node body."
                    ),
                    fix: format!(
                        "Either add '[evidence:{ev_id}]' to the body of the appropriate node, \
                         or remove the @graphite:evidence annotation."
                    ),
                    example: Some(format!(
                        "In a node body:\n\n[evidence:{ev_id}]"
                    )),
                    hint: "Every @graphite:evidence anchor must be referenced by at \
                           least one node's [evidence:...] body reference."
                        .to_string(),
                });
            }
        }

        diagnostics
    }

    /// Check that no node's file exceeds the configured maximum character count.
    /// Rule: `"node-file-too-large"`
    pub fn check_node_file_size(&self, graph: &Graph, max_chars: usize) -> Vec<Diagnostic> {
        graph
            .nodes
            .values()
            .filter(|n| n.content_len > max_chars)
            .map(|n| Diagnostic {
                rule: "node-file-too-large".to_string(),
                severity: Severity::Error,
                node_id: Some(n.id.clone()),
                file: None,
                detail: format!(
                    "Node '{}' has {} characters, which exceeds the maximum of {}. \
                     Split this node into multiple smaller nodes.",
                    n.id, n.content_len, max_chars
                ),
                fix: format!(
                    "Reduce the node body below {} characters by splitting content \
                     across multiple nodes.",
                    max_chars
                ),
                example: Some(
                    "Split your content into separate `.node` files, each covering \
                     one concept, and connect them with edges."
                        .to_string(),
                ),
                hint: "Large nodes indicate too much information in one place. \
                        Split them into smaller, focused nodes connected by edges."
                    .to_string(),
            })
            .collect()
    }

    /// Check that every `[evidence:X]` in a node body resolves to an anchor found
    /// in source code. Scans the body for `[evidence:<id>]` markers and matches
    /// against the resolved evidence map.
    ///
    /// Rules emitted:
    /// - `"unresolved-evidence"` — evidence ID referenced but not found in any source
    pub fn check_evidence_anchors(
        &self,
        graph: &Graph,
        resolved: &ResolvedEvidence,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            let mut seen_ids: HashSet<String> = HashSet::new();

            for ev_id in extract_evidence_refs(&node.body) {
                if !seen_ids.insert(ev_id.clone()) {
                    continue;
                }
                if !resolved.contains_key(&ev_id) {
                    diagnostics.push(Diagnostic {
                        rule: "unresolved-evidence".to_string(),
                        severity: Severity::Error,
                        node_id: Some(node.id.clone()),
                        file: None,
                        detail: format!(
                            "Node '{}' references evidence '{}' but no matching \
                             @graphite:evidence annotation or sidecar pattern was found.",
                            node.id, ev_id
                        ),
                        fix: format!(
                            "Add `// @graphite:evidence {}` to a source file, or create a \
                             `.graphite` sidecar entry for it.",
                            ev_id
                        ),
                        example: Some(
                            "In a .rs file:  // @graphite:evidence my-evid\n\
                             In a .graphite sidecar:  {\"anchors\": {\"my-evid\": \
                             {\"pattern\": \"...\"}}}"
                                .to_string(),
                        ),
                        hint: "Evidence IDs must correspond to @graphite:evidence \
                                annotations in the codebase or patterns in sidecar files."
                            .to_string(),
                    });
                }
            }
        }

        diagnostics
    }
}

// ---------------------------------------------------------------------------
// Helpers: extract [edge:X] and [evidence:X] references from body text
// ---------------------------------------------------------------------------

// @graphite:evidence spec-markdown-extension
/// Scan `body` for markers of the form `[edge:<id>]` and return the
/// captured `<id>` values in order of appearance.
fn extract_edge_refs(body: &str) -> Vec<String> {
    extract_marker_refs(body, "[edge:")
}

/// Scan `body` for markers of the form `[evidence:<id>]` and return the
/// captured `<id>` values in order of appearance.
fn extract_evidence_refs(body: &str) -> Vec<String> {
    extract_marker_refs(body, "[evidence:")
}

/// Generic extractor for `[marker:<id>]` patterns.
fn extract_marker_refs(body: &str, marker: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut pos = 0;
    while let Some(start) = body[pos..].find(marker) {
        let content_start = pos + start + marker.len();
        if let Some(rel_end) = body[content_start..].find(']') {
            let id = body[content_start..content_start + rel_end].trim();
            if !id.is_empty() {
                result.push(id.to_string());
            }
            pos = content_start + rel_end + 1;
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_parser::NodeParser;
    use crate::schema::SchemaParser;
    use crate::Node;

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// A small valid graph with nodes connected via body [edge:X] references.
    fn sample_graph() -> Graph {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: root\n\
category: spec\n\
---\n\
# Root\n\n\
Contains [edge:req-1] and [edge:svc-1].\n",
            )
            .expect("sample root"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: req-1\n\
category: requirement\n\
---\n\
# Requirement\n\n\
Implemented by [edge:svc-1].\n",
            )
            .expect("sample requirement"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc-1\n\
category: service\n\
---\n\
# Service\n",
            )
            .expect("sample service"),
        );

        g
    }

    /// Build a [`Node`] directly, skipping YAML frontmatter parsing.
    fn make_node(id: &str, category: &str, body: &str) -> Node {
        Node {
            id: id.to_string(),
            category: category.to_string(),
            body: body.to_string(),
            content_len: body.len(),
        }
    }

    // ------------------------------------------------------------------
    // Edge refs existence
    // ------------------------------------------------------------------

    #[test]
    fn all_edge_refs_exist() {
        let graph = sample_graph();
        let engine = ValidationEngine;
        let diags = engine.validate(&graph);
        assert!(
            !diags.iter().any(|d| d.rule == "broken-edge-target"),
            "all edge refs should resolve: {:?}",
            diags
        );
    }

    #[test]
    fn broken_edge_ref_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "req-1",
            "requirement",
            "Addressed by [edge:ghost-arc].",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "broken-edge-target"),
            "should detect broken edge target: {:?}",
            diags
        );
    }

    #[test]
    fn existing_edge_ref_passes_silently() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node("arc-1", "architecture", "# Arc One\n\n"));
        g.add_node(make_node(
            "req-1",
            "requirement",
            "Addressed by [edge:arc-1].",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().all(|d| d.rule != "broken-edge-target"),
            "should NOT detect broken edge target when target exists: {:?}",
            diags
        );
    }

    // ------------------------------------------------------------------
    // Node title
    // ------------------------------------------------------------------

    #[test]
    fn node_with_title_passes() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node("svc-1", "service", "# Service\n\nBody."));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            !diags.iter().any(|d| d.rule == "node-body-title"),
            "node with title should pass: {:?}",
            diags
        );
    }

    #[test]
    fn node_without_title_warns() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node("svc-1", "service", "Body without title."));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "node-body-title"),
            "node without title should warn: {:?}",
            diags
        );
    }

    // ------------------------------------------------------------------
    // Evidence anchor resolution
    // ------------------------------------------------------------------

    #[test]
    fn all_evidence_resolved() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "svc-1",
            "service",
            "Service body [evidence:ev-auth]",
        ));

        let mut resolved = ResolvedEvidence::new();
        resolved.insert("ev-auth".into(), vec![(PathBuf::from("src/main.rs"), 42)]);

        let engine = ValidationEngine;
        let diags = engine.check_evidence_anchors(&g, &resolved);
        assert!(
            diags.is_empty(),
            "no diagnostics expected when evidence is resolved: {:?}",
            diags
        );
    }

    #[test]
    fn unresolved_evidence_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "svc-1",
            "service",
            "Service body [evidence:ev-missing]",
        ));

        let resolved = ResolvedEvidence::new(); // empty — nothing resolved

        let engine = ValidationEngine;
        let diags = engine.check_evidence_anchors(&g, &resolved);
        assert_eq!(diags.len(), 1, "exactly one diagnostic expected");
        assert_eq!(diags[0].rule, "unresolved-evidence");
        assert!(
            diags[0].detail.contains("ev-missing"),
            "detail mentions the missing id"
        );
    }

    #[test]
    fn no_evidence_refs_passes_silently() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node("svc-1", "service", "body"));

        let resolved = ResolvedEvidence::new();
        let engine = ValidationEngine;
        let diags = engine.check_evidence_anchors(&g, &resolved);
        assert!(diags.is_empty(), "no evidence refs = no diagnostics");
    }

    #[test]
    fn multiple_evidence_some_unresolved() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "svc-1",
            "service",
            "body [evidence:ev-ok] [evidence:ev-missing]",
        ));

        let mut resolved = ResolvedEvidence::new();
        resolved.insert("ev-ok".into(), vec![(PathBuf::from("main.rs"), 1)]);

        let engine = ValidationEngine;
        let diags = engine.check_evidence_anchors(&g, &resolved);
        assert_eq!(diags.len(), 1, "only the unresolved one should fire");
        assert!(
            diags[0].detail.contains("ev-missing"),
            "detail mentions the missing id: {}",
            diags[0].detail
        );
    }

    #[test]
    fn unused_evidence_anchor_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node("svc-1", "service", "# Service\n"));

        let mut resolved = ResolvedEvidence::new();
        resolved.insert("ev-orphan".into(), vec![(PathBuf::from("main.rs"), 1)]);

        let engine = ValidationEngine;
        let diags = engine.check_unused_anchors(&g, &resolved);
        assert_eq!(diags.len(), 1, "unused anchor should fire");
        assert_eq!(diags[0].rule, "unused-evidence-anchor");
    }

    #[test]
    fn used_evidence_anchor_passes() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "svc-1",
            "service",
            "# Service\n\nBody [evidence:ev-used]",
        ));

        let mut resolved = ResolvedEvidence::new();
        resolved.insert("ev-used".into(), vec![(PathBuf::from("main.rs"), 1)]);

        let engine = ValidationEngine;
        let diags = engine.check_unused_anchors(&g, &resolved);
        assert!(
            diags.is_empty(),
            "used evidence anchor should not produce diagnostics"
        );
    }

    // ------------------------------------------------------------------
    // Extract refs helpers
    // ------------------------------------------------------------------

    #[test]
    fn extract_edge_refs_works() {
        let body = "See [edge:node-a] and [edge:node-b] for details.";
        let refs = extract_edge_refs(body);
        assert_eq!(refs, vec!["node-a", "node-b"]);
    }

    #[test]
    fn extract_evidence_refs_works() {
        let body = "Shown by [evidence:ev-impl] and [evidence:ev-test].";
        let refs = extract_evidence_refs(body);
        assert_eq!(refs, vec!["ev-impl", "ev-test"]);
    }

    #[test]
    fn extract_refs_empty_when_no_markers() {
        assert!(extract_edge_refs("no markers here").is_empty());
        assert!(extract_evidence_refs("no markers here").is_empty());
    }
}
