use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::{Diagnostic, Graph, Severity};

/// A map from evidence ID to resolved file locations.
///
/// Produced by merging results from `AnchorScanner` and `SidecarResolver`.
pub type ResolvedEvidence = HashMap<String, Vec<(PathBuf, usize)>>;

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
        diagnostics.extend(self.check_tree_constraint(graph));
        diagnostics.extend(self.check_cycles(graph));
        diagnostics.extend(self.check_dependency_cycles(graph));
        diagnostics.extend(self.check_schema_conformance(graph));
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
    // Check 2: tree constraint  (rule: "multiple-parents")
    // ------------------------------------------------------------------

    fn check_tree_constraint(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();

        for node in graph.nodes.values() {
            if let Some(targets) = node.edges.get("contains") {
                for target in targets {
                    incoming
                        .entry(target.as_str())
                        .or_default()
                        .push(node.id.as_str());
                }
            }
        }

        incoming
            .into_iter()
            .filter(|(_, parents)| parents.len() > 1)
            .map(|(child, parents)| {
                diag_err(
                    "multiple-parents",
                    child,
                    &format!(
                        "Node '{child}' has {} incoming 'contains' edges: [{}]",
                        parents.len(),
                        parents.join(", ")
                    ),
                    "Remove duplicate 'contains' references so each node has at most one parent.",
                    "The containment graph must be a tree. A node cannot be contained \
                 by multiple index nodes.",
                )
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Check 3: cycles  (rule: "cycle-detected")
    // ------------------------------------------------------------------

    /// DFS with white/gray/black coloring on the `contains` subgraph.
    /// A back edge to a gray node indicates a cycle.
    fn check_cycles(&self, graph: &Graph) -> Vec<Diagnostic> {
        // Map node IDs to dense indices so we can use flat arrays.
        let node_ids: Vec<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();
        let id_to_idx: HashMap<&str, usize> = node_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        let n = node_ids.len();

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for node in graph.nodes.values() {
            if let Some(targets) = node.edges.get("contains") {
                let src = id_to_idx[node.id.as_str()];
                for t in targets {
                    if let Some(&tgt) = id_to_idx.get(t.as_str()) {
                        adj[src].push(tgt);
                    }
                }
            }
        }

        let mut color = vec![0u8; n]; // 0 = white, 1 = gray, 2 = black
        let mut path = Vec::new();
        let mut diagnostics = Vec::new();

        for start in 0..n {
            if color[start] == 0 {
                Self::dfs_cycle(
                    start,
                    &adj,
                    &mut color,
                    &mut path,
                    &node_ids,
                    &mut diagnostics,
                );
            }
        }

        diagnostics
    }

    fn dfs_cycle(
        idx: usize,
        adj: &[Vec<usize>],
        color: &mut [u8],
        path: &mut Vec<usize>,
        node_ids: &[&str],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        color[idx] = 1; // gray
        path.push(idx);

        for &next in &adj[idx] {
            if color[next] == 1 {
                // Back edge: reconstruct cycle description
                let mut cycle: Vec<&str> = path
                    .iter()
                    .skip_while(|&&i| i != next)
                    .map(|&i| node_ids[i])
                    .collect();
                cycle.push(node_ids[next]);

                diagnostics.push(diag_err(
                    "cycle-detected",
                    node_ids[idx],
                    &format!("Cycle detected in containment graph: {}", cycle.join(" → ")),
                    "Remove one of the 'contains' edges to break the cycle.",
                    "The containment graph must be a tree (DAG).",
                ));
            } else if color[next] == 0 {
                Self::dfs_cycle(next, adj, color, path, node_ids, diagnostics);
            }
        }

        color[idx] = 2; // black
        path.pop();
    }

    // ------------------------------------------------------------------
    // Check 4: schema conformance  (rule: "schema-conformance")
    // ------------------------------------------------------------------

    fn check_schema_conformance(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            let is_index = node.kind == "index";

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
                             or change the node kind to a non-index kind.",
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
                    .any(|e| e.from == "any" || e.from == node.kind);
                if !source_ok {
                    let allowed: Vec<&str> = edge_defs.iter().map(|e| e.from.as_str()).collect();
                    diagnostics.push(diag_err(
                        "schema-conformance",
                        &node.id,
                        &format!(
                            "Node '{}' of kind '{}' cannot use edge '{}'. \
                             Expected source kinds: [{}]",
                            node.id,
                            node.kind,
                            edge_kind,
                            allowed.join(", ")
                        ),
                        &format!(
                            "Change the node's kind or use an edge that allows '{}' as the source.",
                            node.kind
                        ),
                        "Each edge type specifies which kinds can be its source ('from' field).",
                    ));
                }

                for target in targets {
                    if let Some(target_node) = graph.nodes.get(target) {
                        let target_ok = edge_defs
                            .iter()
                            .any(|e| e.to == "any" || e.to == target_node.kind);
                        if !target_ok {
                            let allowed: Vec<&str> =
                                edge_defs.iter().map(|e| e.to.as_str()).collect();
                            diagnostics.push(diag_err(
                                "schema-conformance",
                                &node.id,
                                &format!(
                                    "Edge '{}' on node '{}' targets node '{}' of kind '{}'. \
                                     Expected target kinds: [{}]",
                                    edge_kind,
                                    node.id,
                                    target,
                                    target_node.kind,
                                    allowed.join(", ")
                                ),
                                &format!(
                                    "Update the edge target or use an edge \
                                     that allows '{}' as the target.",
                                    target_node.kind
                                ),
                                "Each edge type specifies which kinds can be \
                                 its target ('to' field).",
                            ));
                        }
                    }
                }
            }
        }

        diagnostics
    }

    fn check_body_edge_usage(&self, graph: &Graph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in graph.nodes.values() {
            if node.kind == "index" || node.kind == "evidence" || node.kind == "guide" {
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

    /// Check semantic dependency cycles (non-containment edges) among knowledge
    /// nodes. Cycles in references/describes/etc. are disallowed.
    fn check_dependency_cycles(&self, graph: &Graph) -> Vec<Diagnostic> {
        let node_ids: Vec<&str> = graph
            .nodes
            .values()
            .filter(|n| n.kind != "index" && n.kind != "evidence")
            .map(|n| n.id.as_str())
            .collect();
        let id_to_idx: HashMap<&str, usize> = node_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();

        let n = node_ids.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        for node in graph.nodes.values() {
            if node.kind == "index" || node.kind == "evidence" {
                continue;
            }
            let Some(&src) = id_to_idx.get(node.id.as_str()) else {
                continue;
            };
            for (edge_kind, targets) in &node.edges {
                if edge_kind == "contains" || edge_kind == "evidence" || edge_kind == "describes" {
                    continue;
                }
                for target in targets {
                    if let Some(target_node) = graph.nodes.get(target)
                        && target_node.kind != "index"
                        && target_node.kind != "evidence"
                        && let Some(&tgt) = id_to_idx.get(target.as_str())
                    {
                        adj[src].push(tgt);
                    }
                }
            }
        }

        let mut color = vec![0u8; n];
        let mut path = Vec::new();
        let mut diagnostics = Vec::new();

        for start in 0..n {
            if color[start] == 0 {
                Self::dfs_dependency_cycle(
                    start,
                    &adj,
                    &mut color,
                    &mut path,
                    &node_ids,
                    &mut diagnostics,
                );
            }
        }

        diagnostics
    }

    fn dfs_dependency_cycle(
        idx: usize,
        adj: &[Vec<usize>],
        color: &mut [u8],
        path: &mut Vec<usize>,
        node_ids: &[&str],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        color[idx] = 1;
        path.push(idx);

        for &next in &adj[idx] {
            if color[next] == 1 {
                let mut cycle: Vec<&str> = path
                    .iter()
                    .skip_while(|&&i| i != next)
                    .map(|&i| node_ids[i])
                    .collect();
                cycle.push(node_ids[next]);

                diagnostics.push(diag_err(
                    "dependency-cycle",
                    node_ids[idx],
                    &format!(
                        "Cycle detected in semantic dependencies: {}",
                        cycle.join(" → ")
                    ),
                    "Remove one of the semantic edges to break the cycle.",
                    "Knowledge-node dependencies must be acyclic.",
                ));
            } else if color[next] == 0 {
                Self::dfs_dependency_cycle(next, adj, color, path, node_ids, diagnostics);
            }
        }

        color[idx] = 2;
        path.pop();
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
            if node.kind == "index" || node.kind == "evidence" || node.kind == "guide" {
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
            if node.kind != "index" {
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
                            .get("of_kind")
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
            if node.kind == "index" || node.kind == "evidence" {
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

            for ev_id in evidence_ids {
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
kind: index\n\
edges:\n  contains:\n    - req-1\n    - adr-1\n    - svc-1\n    - tst-1\n\
metadata:\n  of_kind: general\n\
---\n",
            )
            .expect("sample index"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: req-1\n\
kind: requirement\n\
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
kind: adr\n\
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
kind: service\n\
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
kind: test\n\
---\n\
# Test\n",
            )
            .expect("sample test"),
        );

        g
    }

    /// Build a [`Node`] directly, skipping YAML frontmatter parsing.
    fn make_node(id: &str, kind: &str, edges: HashMap<String, Vec<String>>, body: &str) -> Node {
        Node {
            id: id.to_string(),
            kind: kind.to_string(),
            body: body.to_string(),
            edges,
            metadata: HashMap::new(),
            index: if kind == "index" {
                Some(Index {
                    of_kind: "general".to_string(),
                })
            } else {
                None
            },
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
        // Cycle detection also fires.
        assert!(
            diags.iter().any(|d| d.rule == "cycle-detected"),
            "cycle should also be detected"
        );
    }

    // ------------------------------------------------------------------
    // Tree constraint
    // ------------------------------------------------------------------

    #[test]
    fn multiple_parents_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx-1",
            "index",
            HashMap::from([("contains".into(), vec!["svc".into()])]),
            "",
        ));
        g.add_node(make_node(
            "idx-2",
            "index",
            HashMap::from([("contains".into(), vec!["svc".into()])]),
            "",
        ));
        g.add_node(make_node("svc", "service", HashMap::new(), ""));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "multiple-parents"),
            "should detect multiple parents: {:?}",
            diags
        );
        // "svc" has two parents: idx-1 and idx-2
        let mp: Vec<&Diagnostic> = diags
            .iter()
            .filter(|d| d.rule == "multiple-parents")
            .collect();
        assert_eq!(mp.len(), 1, "exactly one multiple-parents diagnostic");
        assert!(
            mp[0].detail.contains("svc"),
            "detail mentions svc: {}",
            mp[0].detail
        );
    }

    // ------------------------------------------------------------------
    // Cycles
    // ------------------------------------------------------------------

    #[test]
    fn cycle_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "x",
            "index",
            HashMap::from([("contains".into(), vec!["y".into()])]),
            "",
        ));
        g.add_node(make_node(
            "y",
            "index",
            HashMap::from([("contains".into(), vec!["z".into()])]),
            "",
        ));
        g.add_node(make_node(
            "z",
            "index",
            HashMap::from([("contains".into(), vec!["x".into()])]),
            "",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "cycle-detected"),
            "should detect cycle: {:?}",
            diags
        );
    }

    #[test]
    fn dependency_cycle_detected() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(make_node(
            "idx",
            "index",
            HashMap::from([("contains".into(), vec!["a".into(), "b".into()])]),
            "# Index\n",
        ));

        g.add_node(make_node(
            "a",
            "service",
            HashMap::from([
                ("references".into(), vec!["b".into()]),
                ("evidence".into(), vec!["ev-a".into()]),
            ]),
            "# A\n\n[edge:b]",
        ));

        g.add_node(make_node(
            "b",
            "service",
            HashMap::from([
                ("references".into(), vec!["a".into()]),
                ("evidence".into(), vec!["ev-b".into()]),
            ]),
            "# B\n\n[edge:a]",
        ));

        let engine = ValidationEngine;
        let diags = engine.validate(&g);
        assert!(
            diags.iter().any(|d| d.rule == "dependency-cycle"),
            "should detect dependency cycle: {:?}",
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

    #[test]
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
kind: adr\n\
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
kind: service\n\
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
kind: adr\n\
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
