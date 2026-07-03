use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphite_core::{Diagnostic, Graph, Node, Severity};
use pulldown_cmark::HeadingLevel;

/// Output of rendering a single node page.
pub struct RenderedNode {
    pub id: String,
    pub kind: String,
    pub html: String,
}

/// Maps each node ID to its containment depth (0 = root).
pub type DepthMap = HashMap<String, usize>;

/// The rendered output for the entire graph.
pub struct RenderedGraph {
    pub pages: Vec<RenderedNode>,
    pub depth_map: DepthMap,
}

/// Errors produced during rendering.
#[allow(clippy::result_large_err)]
pub fn render_to_dir(
    graph: &Graph,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
    output_dir: &Path,
) -> Result<(), Diagnostic> {
    let rendered = render_graph(graph, evidence)?;
    fs::create_dir_all(output_dir).map_err(|e| Diagnostic {
        rule: "render-error".into(),
        severity: Severity::Error,
        node_id: None,
        file: Some(output_dir.to_string_lossy().into()),
        detail: format!(
            "Cannot create output directory '{}': {}",
            output_dir.display(),
            e
        ),
        fix: "Ensure the parent directory exists and is writable.".into(),
        example: None,
        hint: "Output directory must be creatable/writable.".into(),
    })?;

    for page in &rendered.pages {
        let dir = output_dir.join(&page.kind);
        fs::create_dir_all(&dir).ok();
        let file_path = dir.join(format!("{}.html", page.id));
        fs::write(&file_path, &page.html).map_err(|e| Diagnostic {
            rule: "render-error".into(),
            severity: Severity::Error,
            node_id: Some(page.id.clone()),
            file: Some(file_path.to_string_lossy().into()),
            detail: format!("Cannot write '{}': {}", file_path.display(), e),
            fix: "Check directory permissions.".into(),
            example: None,
            hint: "Output directory must be writable.".into(),
        })?;
    }

    // Generate kind index pages
    for (kind, nodes) in group_by_kind(graph) {
        let kind_dir = output_dir.join(&kind);
        fs::create_dir_all(&kind_dir).ok();
        let index_html = render_kind_index(&kind, &nodes, &rendered.depth_map);
        fs::write(kind_dir.join("index.html"), index_html).ok();
    }

    // Generate root index
    let root_html = render_root_index(graph, &rendered.depth_map);
    fs::write(output_dir.join("index.html"), root_html).ok();

    Ok(())
}

/// Render the entire graph to HTML pages.
#[allow(clippy::result_large_err)]
fn render_graph(
    graph: &Graph,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> Result<RenderedGraph, Diagnostic> {
    let depth_map = compute_depths(graph);

    let mut pages = Vec::new();
    for node in graph.nodes.values() {
        let html = render_node_page(graph, node, &depth_map, evidence);
        pages.push(RenderedNode {
            id: node.id.clone(),
            kind: node.kind.clone(),
            html,
        });
    }

    Ok(RenderedGraph { pages, depth_map })
}

// ---------------------------------------------------------------------------
// Containment depth computation
// ---------------------------------------------------------------------------

/// Compute the depth of each node in the containment tree.
///
/// Root nodes (no `contains` edge pointing to them) have depth 0.
/// Each subsequent containment level adds 1.
fn compute_depths(graph: &Graph) -> DepthMap {
    // Determine which nodes are targeted by a `contains` edge
    let mut targeted: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in graph.nodes.values() {
        if let Some(targets) = node.edges.get("contains") {
            for t in targets {
                targeted
                    .entry(t.as_str())
                    .or_default()
                    .push(node.id.as_str());
            }
        }
    }

    let mut depths = HashMap::new();
    let mut queue: Vec<(&str, usize)> = Vec::new();

    // Roots: nodes not targeted by any contains edge
    for id in graph.nodes.keys() {
        if !targeted.contains_key(id.as_str()) {
            depths.insert(id.clone(), 0usize);
            queue.push((id.as_str(), 0));
        }
    }

    // BFS
    while let Some((current, depth)) = queue.pop() {
        if let Some(node) = graph.nodes.get(current)
            && let Some(targets) = node.edges.get("contains")
        {
            for t in targets {
                if !depths.contains_key(t.as_str()) {
                    depths.insert(t.clone(), depth + 1);
                    queue.push((t.as_str(), depth + 1));
                }
            }
        }
    }

    // Any node still missing gets depth 0
    for id in graph.nodes.keys() {
        depths.entry(id.clone()).or_insert(0);
    }

    depths
}

/// Group nodes by their kind.
fn group_by_kind(graph: &Graph) -> HashMap<String, Vec<&Node>> {
    let mut map: HashMap<String, Vec<&Node>> = HashMap::new();
    for node in graph.nodes.values() {
        map.entry(node.kind.clone()).or_default().push(node);
    }
    map
}

// ---------------------------------------------------------------------------
// Single node page rendering
// ---------------------------------------------------------------------------

fn render_node_page(
    graph: &Graph,
    node: &Node,
    depths: &DepthMap,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> String {
    let depth = depths.get(&node.id).copied().unwrap_or(0);
    let heading_base = (1 + depth).min(6); // clamp at h6

    // Build heading-depth-adjusted body
    let body_html = render_body(node, heading_base);

    // Replace [edge:<id>] with links
    let body_with_links = replace_edge_refs(graph, &body_html);

    // Backlinks
    let backlinks = render_backlinks(graph, &node.id);

    // Evidence section
    let ev_section = render_evidence_section(node, evidence);

    // TOC link
    let toc_link = format!("/kind/{}.html", node.kind);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>{id} — graphite</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 1em; line-height: 1.6; }}
a {{ color: #0066cc; }}
a:hover {{ text-decoration: underline; }}
.evidence {{ background: #f5f5f5; padding: 0.5em 1em; border-radius: 4px; }}
.backlinks {{ border-top: 1px solid #ddd; margin-top: 1em; padding-top: 0.5em; }}
.node-meta {{ color: #666; font-size: 0.9em; margin-bottom: 1em; }}
pre {{ background: #f0f0f0; padding: 0.5em; overflow-x: auto; }}
code {{ background: #f0f0f0; padding: 0.1em 0.3em; }}
</style>
</head>
<body>
<p class="node-meta"><strong>{kind}</strong> · <a href="{toc_link}">↑ {kind}</a></p>
{body_with_links}
{ev_section}
{backlinks}
</body>
</html>"#,
        id = node.id,
        kind = node.kind,
        toc_link = toc_link,
        body_with_links = body_with_links,
        ev_section = ev_section,
        backlinks = backlinks,
    )
}

// ---------------------------------------------------------------------------
// Markdown → HTML via pulldown-cmark, with heading offset
// ---------------------------------------------------------------------------

fn render_body(node: &Node, heading_base: usize) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(&node.body, options);

    let offset = if heading_base > 0 {
        heading_base - 1
    } else {
        0
    };

    let mut out = String::new();
    let events = parser.map(|event| match event {
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => {
            let new_level = offset_heading(level, offset);
            Event::Start(Tag::Heading {
                level: new_level,
                id,
                classes,
                attrs,
            })
        }
        Event::End(TagEnd::Heading(level)) => {
            let new_level = offset_heading(level, offset);
            Event::End(TagEnd::Heading(new_level))
        }
        other => other,
    });

    html::push_html(&mut out, events);
    out
}

fn offset_heading(level: HeadingLevel, offset: usize) -> HeadingLevel {
    let n = level as usize + offset;
    match n.min(6) {
        0 | 1 => HeadingLevel::H1,
        2 => HeadingLevel::H2,
        3 => HeadingLevel::H3,
        4 => HeadingLevel::H4,
        5 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

// ---------------------------------------------------------------------------
// [edge:<id>] → <a href="/<kind>/<id>.html">
// ---------------------------------------------------------------------------

fn replace_edge_refs(graph: &Graph, html: &str) -> String {
    let marker = "[edge:";
    let mut result = String::new();
    let mut pos = 0;

    while let Some(start) = html[pos..].find(marker) {
        result.push_str(&html[pos..pos + start]);
        let content_start = pos + start + marker.len();
        if let Some(end) = html[content_start..].find(']') {
            let id = html[content_start..content_start + end].trim();
            if let Some(target) = graph.nodes.get(id) {
                result.push_str(&format!(
                    r#"<a href="/{kind}/{id}.html">{id}</a>"#,
                    kind = target.kind,
                    id = target.id,
                ));
            } else {
                // Target not found — render as text with a broken-link indicator
                result.push_str(&format!(
                    "<span class=\"broken-edge\">{id}?</span>",
                    id = id
                ));
            }
            pos = content_start + end + 1;
        } else {
            result.push_str(marker);
            pos = content_start;
        }
    }

    result.push_str(&html[pos..]);
    result
}

// ---------------------------------------------------------------------------
// Backlinks
// ---------------------------------------------------------------------------

fn render_backlinks(graph: &Graph, node_id: &str) -> String {
    let mut backlinks: Vec<&str> = Vec::new();

    for node in graph.nodes.values() {
        for targets in node.edges.values() {
            if targets.iter().any(|t| t == node_id) && !backlinks.contains(&node.id.as_str()) {
                backlinks.push(node.id.as_str());
            }
        }
    }

    if backlinks.is_empty() {
        return String::new();
    }

    let items: String = backlinks
        .iter()
        .map(|id| {
            format!(
                "<li><a href=\"/{}/{}.html\">{}</a></li>",
                graph
                    .nodes
                    .get(*id)
                    .map(|n| n.kind.as_str())
                    .unwrap_or("unknown"),
                id,
                id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"<div class="backlinks"><h3>Referenced by</h3><ul>{items}</ul></div>"#,)
}

// ---------------------------------------------------------------------------
// Evidence section
// ---------------------------------------------------------------------------

fn render_evidence_section(
    node: &Node,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> String {
    let mut items = String::new();

    for (edge_kind, targets) in &node.edges {
        if edge_kind != "evidence" {
            continue;
        }
        for ev_id in targets {
            if let Some(locations) = evidence.get(ev_id) {
                for (path, line) in locations {
                    items.push_str(&format!(
                        r#"<li><code>{}</code> line {}</li>"#,
                        path.display(),
                        line
                    ));
                }
            } else {
                items.push_str(&format!(
                    r#"<li><code>{}</code> <em>(unresolved)</em></li>"#,
                    ev_id
                ));
            }
        }
    }

    if items.is_empty() {
        return String::new();
    }

    format!(r#"<div class="evidence"><h3>Evidence</h3><ul>{items}</ul></div>"#,)
}

// ---------------------------------------------------------------------------
// Kind index page
// ---------------------------------------------------------------------------

fn render_kind_index(kind: &str, nodes: &[&Node], depths: &DepthMap) -> String {
    let mut items: Vec<String> = nodes
        .iter()
        .map(|n| {
            let depth = depths.get(&n.id).copied().unwrap_or(0);
            let indent = "  ".repeat(depth);
            format!(r#"{}<li><a href="{}.html">{}</a></li>"#, indent, n.id, n.id)
        })
        .collect();
    items.sort();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>{kind} — graphite</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 1em; line-height: 1.6; }}
a {{ color: #0066cc; }}
</style>
</head>
<body>
<h1>{kind}</h1>
<p>Nodes of kind <strong>{kind}</strong>.</p>
<ul>
{items}
</ul>
<a href="/index.html">← Graph root</a>
</body>
</html>"#,
        kind = kind,
        items = items.join("\n"),
    )
}

// ---------------------------------------------------------------------------
// Root index page
// ---------------------------------------------------------------------------

fn render_root_index(graph: &Graph, depths: &DepthMap) -> String {
    let mut kind_links = String::new();
    let mut seen_kinds: Vec<&str> = graph.nodes.values().map(|n| n.kind.as_str()).collect();
    seen_kinds.sort();
    seen_kinds.dedup();

    for kind in &seen_kinds {
        kind_links.push_str(&format!(
            r#"<li><a href="{kind}/index.html">{kind}</a></li>"#,
            kind = kind,
        ));
    }

    let mut all_items: Vec<String> = graph
        .nodes
        .values()
        .map(|n| {
            let depth = depths.get(&n.id).copied().unwrap_or(0);
            let indent = "  ".repeat(depth);
            format!(
                r#"{}<li><a href="{kind}/{id}.html">{id}</a></li>"#,
                indent,
                kind = n.kind,
                id = n.id,
            )
        })
        .collect();
    all_items.sort();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>graphite — knowledge graph</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 1em; line-height: 1.6; }}
a {{ color: #0066cc; }}
</style>
</head>
<body>
<h1>graphite</h1>
<p>A compiled knowledge graph for software engineering.</p>
<h2>Kinds</h2>
<ul>
{kind_links}
</ul>
<h2>All Nodes</h2>
<ul>
{all_items}
</ul>
</body>
</html>"#,
        kind_links = kind_links,
        all_items = all_items.join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphite_core::Graph;
    use graphite_core::node_parser::NodeParser;
    use graphite_core::schema::SchemaParser;

    fn sample_graph() -> Graph {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: root\n\
kind: index\n\
edges:\n  contains:\n    - svc\n\
metadata:\n  of_kind: general\n\
---\n\
# Root\n\n\
Root body.\n",
            )
            .expect("root"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc\n\
kind: service\n\
---\n\
# Service\n\n\
Body with [edge:root].\n",
            )
            .expect("svc"),
        );

        g
    }

    #[test]
    fn heading_depth_root_is_h1() {
        let g = sample_graph();
        let depths = compute_depths(&g);
        assert_eq!(depths.get("root"), Some(&0));
        assert_eq!(depths.get("svc"), Some(&1));
    }

    #[test]
    fn renders_without_crashing() {
        let g = sample_graph();
        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence).expect("render should succeed");
        assert_eq!(rendered.pages.len(), 2);

        // Root page should contain h1 since depth=0 → heading_base=1
        let root_page = rendered.pages.iter().find(|p| p.id == "root").unwrap();
        assert!(
            root_page.html.contains("<h1"),
            "root page should have a top-level heading"
        );

        // Service page should have depth=1 → heading_base=2 → h2
        let svc_page = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        assert!(
            svc_page.html.contains("<h2"),
            "child page heading should be offset"
        );
    }

    #[test]
    fn edge_refs_replaced_in_html() {
        let g = sample_graph();
        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence).expect("render");

        let svc = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        // [edge:root] should become a link to root
        assert!(
            svc.html.contains(r#"href="/index/root.html""#),
            "edge ref should be an anchor: {}",
            svc.html
        );
    }

    #[test]
    fn broken_edge_renders_as_span() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: test\n\
kind: service\n\
---\n\
# Test\n\n\
See [edge:nonexistent].\n",
            )
            .expect("test"),
        );

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence).expect("render");
        let page = &rendered.pages[0];
        assert!(
            page.html.contains("nonexistent?"),
            "broken edge should show indicator"
        );
    }

    #[test]
    fn evidence_section_rendered() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc\n\
kind: service\n\
edges:\n  evidence:\n    - ev-auth\n---\n\
# Service\n",
            )
            .expect("svc"),
        );

        let mut evidence = HashMap::new();
        evidence.insert("ev-auth".into(), vec![(PathBuf::from("src/main.rs"), 42)]);

        let rendered = render_graph(&g, &evidence).expect("render");
        let page = &rendered.pages[0];
        assert!(
            page.html.contains("Evidence"),
            "evidence heading should exist"
        );
        assert!(
            page.html.contains("src/main.rs"),
            "evidence file path should appear"
        );
    }

    #[test]
    fn depth_clamped_at_h6() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        // Create a deeply nested chain
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: r0\n\
kind: index\n\
edges:\n  contains:\n    - r1\n\
metadata:\n  of_kind: general\n\
---\n# R0\n",
            )
            .expect("r0"),
        );
        for i in 1..=10 {
            let id = format!("r{i}");
            let next = i + 1;
            g.add_node(
                NodeParser::parse(&format!(
                    "\
---\n\
id: {id}\n\
kind: index\n\
edges:\n  contains:\n    - r{next}\n\
metadata:\n  of_kind: general\n\
---\n# Node {i}\n"
                ))
                .unwrap_or_else(|_| panic!("r{i}")),
            );
        }
        // Last node has no contains
        let last = format!("r11");
        g.add_node(
            NodeParser::parse(&format!(
                "\
---\n\
id: {last}\n\
kind: service\n\
---\n# Last\n"
            ))
            .expect("last"),
        );

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence).expect("render");

        // Deepest node should have heading clamped at h6
        let deepest = rendered.pages.iter().find(|p| p.id == "r11").unwrap();
        assert!(
            deepest.html.contains("<h6"),
            "deepest node should be h6, not lower"
        );
        // Should NOT have h7
        assert!(!deepest.html.contains("<h7"), "no h7 allowed");
    }
}
