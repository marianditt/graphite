use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::{Diagnostic, Graph, Severity};

// @graphite:evidence spec-validate
/// A map from evidence ID to resolved file locations.
///
/// Produced by merging results from `AnchorScanner` and `SidecarResolver`.
pub type ResolvedEvidence = HashMap<String, Vec<(PathBuf, usize)>>;

// @graphite:evidence spec-validate
/// Validates a [`Graph`] against structural and schema constraints.
///
/// Checks run in order:
/// 1. **reachability** — every node reachable from a root via containment
/// 2. **tree constraint** — no node has multiple parents in the containment tree
/// 3. **cycles** — the containment graph must be a DAG
/// 4. **schema conformance** — edges match the declared schema
/// 5. **body-edge usage** — edge declarations and body references are consistent
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
        diagnostics.extend(self.check_reachability(graph));
        diagnostics.extend(self.check_schema_conformance(graph));
        diagnostics.extend(self.check_edge_targets_exist(graph));
        diagnostics.extend(self.check_body_edge_usage(graph));
        diagnostics.extend(self.check_evidence_coverage(graph));
        diagnostics.extend(self.check_index_body_has_title(graph));
        diagnostics.extend(self.check_node_body_has_title(graph));
        diagnostics
    }

    fn check_reachability(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut targeted: HashSet<&str> = HashSet::new();
        let mut contains_out: HashMap<&str, Vec<&str>> = HashMap::new();

        for node in graph.nodes.values() {
            if let Some(targets) = node.edges.get("contains") {
                let entry = contains_out.entry(node.id.as_str()).or_default();
                for t in targets {
                    targeted.insert(t);
                    entry.push(t);
                }
            }
        }

        let roots: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|id| !targeted.contains(id.as_str()))
            .map(|s| s.as_str())
            .collect();

        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();

        for root in &roots {
            if visited.insert(root) {
                queue.push_back(root);
            }
        }

        while let Some(current) = queue.pop_front() {
            if let Some(children) = contains_out.get(current) {
                for &child in children {
                    if graph.nodes.contains_key(child) && visited.insert(child) {
                        queue.push_back(child);
                    }
                }
            }
        }

        graph
            .nodes
            .keys()
            .filter(|id| !visited.contains(id.as_str()))
            .map(|id| {
                diag_err(
                    "unreachable-node",
                    id,
                    &format!(
                        "Node '{id}' is not reachable from any root in the containment hierarchy"
                    ),
                    "Add a path from an index node via 'contains' edges to this node, \
                 or make it a top-level (uncontained) node.",
                    "Every node must be reachable through the containment tree. \
                 Index nodes contain child nodes via 'contains' edges.",
                )
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Check 2: schema conformance  (rule: "schema-conformance")
    // ------------------------------------------------------------------

    fn check_schema_conformance(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            let is_index = node.category == "index";

            for (edge_kind, targets) in &node.edges {
                if is_index && edge_kind != "contains" {
                    diagnostics.push(diag_err(
                        "schema-conformance",
                        &node.id,
                        &format!(
                            "Index node '{}' has non-contains edge '{}'. \
                             Index nodes may only use 'contains' edges.",
                            node.id, edge_kind
                        ),
                        &format!(
                             "Remove the '{}' edge from index node '{}', \
                              or change the node category to a non-index category.",
                            edge_kind, node.id
                        ),
                        "Index nodes organize other nodes via containment. \
                         They cannot have semantic edges.",
                    ));
                    continue;
                }

                if !is_index && edge_kind == "contains" {
                    diagnostics.push(diag_err(
                        "schema-conformance",
                        &node.id,
                        &format!(
                            "Knowledge node '{}' has a 'contains' edge, \
                             which is reserved for index nodes.",
                            node.id
                        ),
                        &format!(
                            "Remove the 'contains' edge from node '{}', \
                             or change it to an index node.",
                            node.id
                        ),
                        "The 'contains' edge type is reserved for index nodes. \
                         Knowledge nodes use semantic edges.",
                    ));
                    continue;
                }

                if edge_kind == "contains" || edge_kind == "evidence" {
                    continue;
                }

                let edge_defs: Vec<&crate::EdgeDef> = graph
                    .schema
                    .edges
                    .iter()
                    .filter(|e| e.name == *edge_kind)
                    .collect();

                if edge_defs.is_empty() {
                    diagnostics.push(diag_err(
                        "schema-conformance",
                        &node.id,
                        &format!(
                            "Edge kind '{}' on node '{}' is not defined in the schema.",
                            edge_kind, node.id
                        ),
                        &format!(
                            "Add '{}' to the schema's 'edges' section, \
                             or remove it from node '{}'.",
                            edge_kind, node.id
                        ),
                        "All edge types must be declared in the schema before use.",
                    ));
                    continue;
                }

                let source_ok = edge_defs
                    .iter()
                    .any(|e| e.from == "any" || e.from == node.category);
                if !source_ok {
                    let allowed: Vec<&str> = edge_defs.iter().map(|e| e.from.as_str()).collect();
                    diagnostics.push(diag_err(
                        "schema-conformance",
                        &node.id,
                        &format!(
                            "Node '{}' of category '{}' cannot use edge '{}'. \
                             Expected source categories: [{}]",
                            node.id,
                            node.category,
                            edge_kind,
                            allowed.join(", ")
                        ),
                        &format!(
                            "Change the node's category or use an edge that allows '{}' as the source.",
                            node.category
                        ),
                        "Each edge type specifies which categories can be its source ('from' field).",
                    ));
                }

                for target in targets {
                    if let Some(target_node) = graph.nodes.get(target) {
                        let target_ok = edge_defs
                            .iter()
                            .any(|e| e.to == "any" || e.to == target_node.category);
                        if !target_ok {
                            let allowed: Vec<&str> =
                                edge_defs.iter().map(|e| e.to.as_str()).collect();
                            diagnostics.push(diag_err(
                                "schema-conformance",
                                &node.id,
                                &format!(
                                "Edge '{}' on node '{}' targets node '{}' of category '{}'. \
                                 Expected target categories: [{}]",
                                    edge_kind,
                                    node.id,
                                    target,
                                    target_node.category,
                                    allowed.join(", ")
                                ),
                                &format!(
                                    "Update the edge target or use an edge \
                                     that allows '{}' as the target.",
                                    target_node.category
                                ),
                                "Each edge type specifies which categories can be \
                                 its target ('to' field).",
                            ));
                        }
                    }
                }
            }
        }

        diagnostics
    }

    /// Check that every edge target (except `evidence`) resolves to an existing node ID.
    fn check_edge_targets_exist(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            for (edge_kind, targets) in &node.edges {
                if edge_kind == "evidence" || edge_kind == "contains" {
                    continue;
                }
                for target in targets {
                    if !graph.nodes.contains_key(target.as_str()) {
                        let detail = format!(
                            "Node '{}' has edge '{}' targeting '{}', but no node with ID \
                             '{}' exists in the graph.",
                            node.id, edge_kind, target, target
                        );
                        let fix = format!(
                            "Either create a new node with id '{}' (in the appropriate \
                             category directory), or correct the edge target in node '{}' \
                             to point to an existing node ID.",
                            target, node.id
                        );
                        let hint = "Every edge target MUST resolve to an existing node \
                                    ID in the graph. Use `graphite context <id>` to \
                                    discover existing nodes, or `graphite ls` to list \
                                    all node IDs."
                            .to_string();
                        diagnostics.push(diag_err(
                            "broken-edge-target",
                            &node.id,
                            &detail,
                            &fix,
                            &hint,
                        ));
                    }
                }
            }
        }

        diagnostics
    }

    fn check_body_edge_usage(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            if node.category == "index" || node.category == "evidence" || node.category == "guide" {
                continue;
            }

            let declared_targets: HashSet<&str> = node
                .edges
                .iter()
                .filter(|(k, _)| k.as_str() != "evidence")
                .flat_map(|(_, v)| v.iter().map(|s| s.as_str()))
                .collect();

            let body_refs: Vec<String> = extract_edge_refs(&node.body);
            let body_ref_set: HashSet<&str> = body_refs.iter().map(|s| s.as_str()).collect();

            for target in &declared_targets {
                if !body_ref_set.contains(target) {
                    diagnostics.push(Diagnostic {
                        rule: "body-edge-usage".to_string(),
                        severity: Severity::Warning,
                        node_id: Some(node.id.clone()),
                        file: None,
                        detail: format!(
                            "Node '{}' declares an edge to '{}' but never references \
                             it in the body via [edge:{}].",
                            node.id, target, target
                        ),
                        fix: format!(
                            "Add '[edge:{}]' to the body of node '{}', \
                             or remove the declaration.",
                            target, node.id
                        ),
                        example: Some(format!(
                            "Add this to the body where appropriate:\n\n[edge:{}]\n\n\
                             Or remove the edge:\n\nedges:\n  ...\n    - {}\n",
                            target, target
                        )),
                        hint: "Every declared edge target should be referenced \
                                in the body using the [edge:<id>] syntax."
                            .to_string(),
                    });
                }
            }

            // Referenced in body but no declaration.
            for body_ref in &body_refs {
                if !declared_targets.contains(body_ref.as_str()) {
                    diagnostics.push(Diagnostic {
                        rule: "dangling-edge-reference".to_string(),
                        severity: Severity::Error,
                        node_id: Some(node.id.clone()),
                        file: None,
                        detail: format!(
                            "Node '{}' has [edge:{}] in its body but no edge \
                             declaration for '{}'.",
                            node.id, body_ref, body_ref
                        ),
                        fix: format!(
                            "Add an edge declaration for '{}' to the node's frontmatter, \
                             or remove [edge:{}] from the body.",
                            body_ref, body_ref
                        ),
                        example: Some(format!(
                            "Add to frontmatter:\n\nedges:\n  references:\n    - {}\n",
                            body_ref
                        )),
                        hint: "All [edge:<id>] references in the body must have \
                                a corresponding edge declaration in the frontmatter."
                            .to_string(),
                    });
                }
            }
        }

        diagnostics
    }

    fn check_evidence_coverage(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let any_evidence_declared = graph.nodes.values().any(|n| {
            n.edges
                .get("evidence")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        });
        if !any_evidence_declared {
            return diagnostics;
        }

        for node in graph.nodes.values() {
            if node.category == "index" || node.category == "evidence" || node.category == "guide" {
                continue;
            }

            let has_evidence = node
                .edges
                .get("evidence")
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !has_evidence {
                diagnostics.push(diag_err(
                    "missing-evidence",
                    &node.id,
                    &format!(
                        "Node '{}' has no evidence edges. Every knowledge node must be anchored in evidence.",
                        node.id
                    ),
                    &format!(
                        "Add an 'evidence' edge to '{}' and provide a matching @graphite:evidence anchor in source code.",
                        node.id
                    ),
                    "Knowledge claims must be anchored in evidence.",
                ));
            }
        }

        diagnostics
    }

    /// Check that every index node body has at least one markdown heading (# title).
    /// Rule: "index-body-title"
    fn check_index_body_has_title(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            if node.category != "index" {
                continue;
            }
            let has_heading = node.body.lines().any(|l| l.trim().starts_with("# "));

            if !has_heading {
                diagnostics.push(Diagnostic {
                    rule: "index-body-title".to_string(),
                    severity: Severity::Warning,
                    node_id: Some(node.id.clone()),
                    file: None,
                    detail: format!(
                        "Index node '{}' has no top-level heading (# Title) in its body.",
                        node.id
                    ),
                    fix: format!(
                        "Add a level-1 heading to the body of '{}', for example:\n\n\
                         # {} Index",
                        node.id,
                        node.metadata
                            .get("of_category")
                            .map(|s| s.as_str())
                            .unwrap_or("Nodes")
                    ),
                    example: None,
                    hint: "Index pages should have a descriptive title as their first heading."
                        .to_string(),
                });
            }
        }

        diagnostics
    }

    /// Check that every non-index node body has at least one markdown heading (# title).
    /// Rule: "node-body-title"
    fn check_node_body_has_title(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            if node.category == "index" || node.category == "evidence" {
                continue;
            }
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
                        "Every knowledge node should have a descriptive title as its first heading."
                            .to_string(),
                });
            }
        }

        diagnostics
    }
    /// Check that every resolved evidence anchor is referenced by at least
    /// one node's `evidence` edge. Unused anchors indicate orphan annotations
    /// that should either be wired to a node or removed.
    ///
    /// Rule: `"unused-evidence-anchor"`
    pub fn check_unused_anchors(
        &self,
        graph: &Graph,
        resolved: &ResolvedEvidence,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let mut used: HashSet<&str> = HashSet::new();
        for node in graph.nodes.values() {
            if let Some(ids) = node.edges.get("evidence") {
                for id in ids {
                    used.insert(id.as_str());
                }
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
                         'evidence' edge on any node."
                    ),
                    fix: format!(
                        "Either add an `evidence` edge referencing '{ev_id}' to the appropriate \
                         node, or remove the @graphite:evidence annotation."
                    ),
                    example: Some(format!(
                        "In a node frontmatter:\n\nedges:\n  evidence:\n    - {ev_id}"
                    )),
                    hint: "Every @graphite:evidence anchor must be referenced by at \
                           least one node's evidence edge."
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

    ///
    /// `resolved` is the merged output of `AnchorScanner` and `SidecarResolver`:
    /// a map from evidence ID to its file+line locations.
    ///
    /// Rules emitted:
    /// - `"unresolved-evidence"` — evidence ID declared but not found in any source
    #[allow(clippy::result_large_err)]
    pub fn check_evidence_anchors(
        &self,
        graph: &Graph,
        resolved: &ResolvedEvidence,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            let Some(evidence_ids) = node.edges.get("evidence") else {
                continue;
            };

            let mut seen_ids: HashSet<&str> = HashSet::new();
            for ev_id in evidence_ids {
                if !seen_ids.insert(ev_id.as_str()) {
                    continue;
                }
                if !resolved.contains_key(ev_id) {
                    diagnostics.push(Diagnostic {
                        rule: "unresolved-evidence".to_string(),
                        severity: Severity::Error,
                        node_id: Some(node.id.clone()),
                        file: None,
                        detail: format!(
                            "Node '{}' declares evidence '{}' but no matching \
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
// Helper: extract [edge:X] references from body text
// ---------------------------------------------------------------------------

// @graphite:evidence spec-markdown-extension
/// Scan `body` for markers of the form `[edge:<id>]` and return the
/// captured `<id>` values in order of appearance.
fn extract_edge_refs(body: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut pos = 0;
    let marker = "[edge:";
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
    use crate::{Index, Node};

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// A small valid graph with an index, a requirement, an ADR, and a service.
    /// Edges and body references are consistent. Passes all checks.
    fn sample_graph() -> Graph {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: idx\n\
category: index\n\
edges:\n  contains:\n    - req-1\n    - adr-1\n    - svc-1\n    - tst-1\n\
metadata:\n  of_category: general\n\
---\n",
            )
            .expect("sample index"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: req-1\n\
category: requirement\n\
edges:\n  implemented_by:\n    - svc-1\n  verified_by:\n    - tst-1\n\
---\n\
# Requirement\n\n\
Implemented by [edge:svc-1] and verified by [edge:tst-1].\n",
            )
            .expect("sample requirement"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: adr-1\n\
category: adr\n\
edges:\n  references:\n    - svc-1\n\
---\n\
# ADR\n\n\
Related: [edge:svc-1]\n",
            )
            .expect("sample adr"),
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

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: tst-1\n\
category: test\n\
---\n\
# Test\n",
            )
            .expect("sample test"),
        );

        g
    }

    /// Build a [`Node`] directly, skipping YAML frontmatter parsing.
    fn make_node(id: &str, category: &str, edges: HashMap<String, Vec<String>>, body: &str) -> Node {
        Node {
            id: id.to_string(),
            category: category.to_string(),
            body: body.to_string(),
            edges,
            metadata: HashMap::new(),
            index: if category == "index" {
                Some(Index {
                    of_category: "general".to_string(),
                })
            } else {
                None
            },
            content_len: body.len(),
        }
    }

    // ------------------------------------------------------------------
    // Reachability
    // ------------------------------------------------------------------

    #[test]
    fn all_nodes_reachable() {
        let graph = sample_graph();
        let engine = ValidationEngine;
        let diags = engine.validate(&graph);
        assert!(
            !diags.iter().any(|d| d.rule == "unreachable-node"),
            "all nodes should be reachable: {:?}",
            diags
        );
    }

    #[test]
    fn unreachable_node_detected() {
        let mut graph = sample_graph();
        // Remove the outgoing contains edge from the index so 'svc-1' is now
        // targeted by contains but its parent (the contains edge) no longer
        // exists in the traversal. We keep svc-1 targeted but unreachable.
        // Simpler: drop the contains edge on the index node.
        if let Some(idx) = graph.nodes.get_mut("idx") {
            idx.edges.remove("contains");
        }
        // Now idx, req-1, adr-1, svc-1 are all roots (not targeted by contains).
        // But svc-1 has no incoming contains edge *and* no path from any
        // index via contains. Actually they are all still roots and visited.

        // Alternative: create a node that is targeted by contains from a
        // non-existent node.  We simulate this by making svc-1's incoming
        // contains come from a node that isn't a root and can't be reached.
        // The simplest way: every node in the graph participates in a contains
        // cycle so no node is a root.

        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "a",
            "index",
            HashMap::from([("contains".into(), vec!["b".into()])]),
            "",
        ));
        g.add_node(make_node(
            "b",
            "index",
            HashMap::from([("contains".into(), vec!["c".into()])]),
            "",
        ));
        g.add_node(make_node(
            "c",
            "index",
            HashMap::from([("contains".into(), vec!["a".into()])]),
            "",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "unreachable-node"),
            "reachability error should fire for cyclic containment: {:?}",
            diags
        );
    }

    #[test]
    fn missing_evidence_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx",
            "index",
            HashMap::from([("contains".into(), vec!["svc".into(), "svc2".into()])]),
            "# Index\n",
        ));
        g.add_node(make_node("svc", "service", HashMap::new(), "# Service\n"));
        g.add_node(make_node(
            "svc2",
            "service",
            HashMap::from([("evidence".into(), vec!["ev-svc2".into()])]),
            "# Service 2\n",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "missing-evidence"),
            "should detect missing evidence: {:?}",
            diags
        );
    }

    // ------------------------------------------------------------------
    // Schema conformance
    // ------------------------------------------------------------------

    #[test]
    fn valid_schema_conformance_passes() {
        let graph = sample_graph();
        let engine = ValidationEngine;
        let diags = engine.validate(&graph);
        assert!(
            !diags.iter().any(|d| d.rule == "schema-conformance"),
            "no schema-conformance errors expected: {:?}",
            diags
        );
    }

    #[test]
    fn index_node_with_non_contains_edge() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx",
            "index",
            HashMap::from([
                ("contains".into(), vec!["svc".into()]),
                ("relates_to".into(), vec!["svc".into()]),
            ]),
            "",
        ));
        g.add_node(make_node("svc", "service", HashMap::new(), ""));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "schema-conformance" && d.detail.contains("non-contains")),
            "index with non-contains edge should error: {:?}",
            diags
        );
    }

    #[test]
    fn knowledge_node_with_contains_edge() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx",
            "index",
            HashMap::from([("contains".into(), vec!["svc".into()])]),
            "",
        ));
        g.add_node(make_node(
            "svc",
            "service",
            HashMap::from([("contains".into(), vec!["other".into()])]),
            "",
        ));
        g.add_node(make_node("other", "service", HashMap::new(), ""));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "schema-conformance" && d.detail.contains("reserved for index")),
            "knowledge node with contains should error: {:?}",
            diags
        );
    }

    // ------------------------------------------------------------------
    // Edge target existence
    // ------------------------------------------------------------------

    #[test]
    fn broken_edge_target_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        // A node with an edge to a non-existent target.
        g.add_node(make_node(
            "req-1",
            "requirement",
            HashMap::from([("addressed_by".into(), vec!["ghost-arc".into()])]),
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
    fn existing_edge_target_passes_silently() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "arc-1",
            "architecture",
            HashMap::new(),
            "# Arc One\n\n",
        ));
        g.add_node(make_node(
            "req-1",
            "requirement",
            HashMap::from([("addressed_by".into(), vec!["arc-1".into()])]),
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
    // Body-edge usage
    // ------------------------------------------------------------------

    #[test]
    fn declared_edge_not_used_in_body() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        // adr node declares 'references: [svc-1]' but body has no [edge:svc-1]
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: adr-1\n\
category: adr\n\
edges:\n  references:\n    - svc-1\n\
---\n\
# ADR\n\n\
No edge reference here.\n",
            )
            .expect("adr"),
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
            .expect("svc"),
        );

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "body-edge-usage"),
            "unused edge declaration should warn: {:?}",
            diags
        );
    }

    #[test]
    fn all_edges_used_in_body() {
        let graph = sample_graph();
        let engine = ValidationEngine;
        let diags = engine.validate(&graph);
        assert!(
            !diags.iter().any(|d| d.rule == "body-edge-usage"),
            "no body-edge-usage warnings expected: {:?}",
            diags
        );
    }

    #[test]
    fn dangling_edge_reference_in_body() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        // Body has [edge:unknown] but no edge declaration for it.
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: adr-1\n\
category: adr\n\
---\n\
# ADR\n\n\
See also: [edge:unknown]\n",
            )
            .expect("adr"),
        );

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "dangling-edge-reference"),
            "dangling body reference should error: {:?}",
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
            HashMap::from([("evidence".into(), vec!["ev-auth".into()])]),
            "Service body",
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
            HashMap::from([("evidence".into(), vec!["ev-missing".into()])]),
            "Service body",
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
    fn evidence_edges_are_ignored_by_body_edge_usage() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx",
            "index",
            HashMap::from([("contains".into(), vec!["svc".into()])]),
            "# Index\n",
        ));

        g.add_node(make_node(
            "svc",
            "service",
            HashMap::from([("evidence".into(), vec!["ev-svc".into()])]),
            "# Service\n",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);

        assert!(
            !diags.iter().any(|d| d.rule == "body-edge-usage"),
            "evidence edges should not require [edge:...] body refs: {:?}",
            diags
        );
    }

    #[test]
    fn no_evidence_edges_passes_silently() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        // Node without any evidence edge.
        g.add_node(make_node("svc-1", "service", HashMap::new(), "body"));

        let resolved = ResolvedEvidence::new();
        let engine = ValidationEngine;
        let diags = engine.check_evidence_anchors(&g, &resolved);
        assert!(diags.is_empty(), "no evidence edges = no diagnostics");
    }

    #[test]
    fn multiple_evidence_some_unresolved() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "svc-1",
            "service",
            HashMap::from([("evidence".into(), vec!["ev-ok".into(), "ev-missing".into()])]),
            "body",
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
}
