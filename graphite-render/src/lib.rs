use std::collections::{HashMap};
use std::fs;
use std::path::{Path};

use graphite_core::{Diagnostic, Graph, Node, Schema, Severity};

pub mod style;

/// Output of rendering a single node page.
pub struct RenderedNode {
    pub id: String,
    pub category: String,
    pub html: String,
}

/// The rendered output for the entire graph.
pub struct RenderedGraph {
    pub pages: Vec<RenderedNode>,
}

/// Per-category sequential numbering: category → node_id → 1-based index.
type NodeNumbering = HashMap<String, HashMap<String, usize>>;

// @graphite:evidence spec-render
/// Errors produced during rendering.
#[allow(clippy::result_large_err)]
pub fn render_to_dir(
    graph: &Graph,
    output_dir: &Path,
    repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> Result<(), Diagnostic> {
    let rendered = render_graph(graph, repo_url, css, base_url)?;
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
        let dir = output_dir.join(&page.category);
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

    // Generate root index page
    let numbering = compute_node_numbering(graph);
    let root_html = render_root_index(graph, &graph.schema, &numbering, css, base_url);
    fs::write(output_dir.join("index.html"), root_html).ok();

    Ok(())
}

/// Render the entire graph to HTML pages.
#[allow(clippy::result_large_err)]
fn render_graph(
    graph: &Graph,
    repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> Result<RenderedGraph, Diagnostic> {
    let _numbering = compute_node_numbering(graph);

    let mut pages = Vec::new();
    for node in graph.nodes.values() {
        let html = render_node_page(graph, node, repo_url, css, base_url);
        pages.push(RenderedNode {
            id: node.id.clone(),
            category: node.category.clone(),
            html,
        });
    }

    Ok(RenderedGraph { pages })
}

/// Group nodes by their category.
fn group_by_category(graph: &Graph) -> HashMap<String, Vec<&Node>> {
    let mut map: HashMap<String, Vec<&Node>> = HashMap::new();
    for node in graph.nodes.values() {
        map.entry(node.category.clone()).or_default().push(node);
    }
    map
}

/// Compute sequential numbering per category (sorted by node ID).
fn compute_node_numbering(graph: &Graph) -> NodeNumbering {
    let mut numbering = HashMap::new();
    for (category, mut nodes) in group_by_category(graph) {
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut cat_map = HashMap::new();
        for (i, node) in nodes.iter().enumerate() {
            cat_map.insert(node.id.clone(), i + 1);
        }
        numbering.insert(category, cat_map);
    }
    numbering
}

/// Build the numeric key label for a node, e.g. "SVC-3".
fn node_key_index(schema: &Schema, numbering: &NodeNumbering, category: &str, node_id: &str) -> String {
    let key = schema
        .categories
        .get(category)
        .map(|k| k.key.as_str())
        .unwrap_or("??");
    let num = numbering
        .get(category)
        .and_then(|m| m.get(node_id))
        .copied()
        .unwrap_or(0);
    format!("{}-{}", key, num)
}

fn node_title(node: &Node) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(&node.body, options);

    let mut in_heading = false;
    let mut title = String::new();
    for event in parser {
        match event {
            Event::Start(Tag::Heading { .. }) if !in_heading && title.is_empty() => {
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) if in_heading => {
                break;
            }
            Event::Text(text) | Event::Code(text) if in_heading => {
                if !title.is_empty() {
                    title.push(' ');
                }
                title.push_str(text.as_ref());
            }
            _ => {}
        }
    }

    if title.trim().is_empty() {
        node.id.clone()
    } else {
        title.trim().to_string()
    }
}

fn node_display_label(schema: &Schema, numbering: &NodeNumbering, node: &Node) -> String {
    let key_index = node_key_index(schema, numbering, &node.category, &node.id);
    let title = node_title(node);
    format!("{} {}", key_index, title)
}

// ---------------------------------------------------------------------------
// Relative link helpers
// ---------------------------------------------------------------------------

/// Build a link from a page of `from_category` to a node in `to_category`.
///
/// When `base_url` is non-empty, generates an absolute path prefixed with
/// `base_url` (for GitHub Pages subpath serving). When empty, generates a
/// relative path (default behaviour).
fn relative_link(base_url: &str, from_category: &str, to_category: &str, to_id: &str) -> String {
    if base_url.is_empty() {
        if from_category == to_category {
            format!("{}.html", to_id)
        } else {
            format!("../{}/{}.html", to_category, to_id)
        }
    } else {
        let base = base_url.trim_end_matches('/');
        format!("{}/{}/{}.html", base, to_category, to_id)
    }
}

/// Build a relative link to the root index page (index.html).
fn root_index_link(base_url: &str, from_category: &str) -> String {
    if base_url.is_empty() {
        if from_category == "index" {
            "index.html".into()
        } else {
            "../index.html".into()
        }
    } else {
        let base = base_url.trim_end_matches('/');
        format!("{}/index.html", base)
    }
}

// ---------------------------------------------------------------------------
// Single node page rendering
// ---------------------------------------------------------------------------

fn render_node_page(
    graph: &Graph,
    node: &Node,
    _repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> String {
    let current_category = &node.category;

    // Build the body (no heading offset without containment depth)
    let key_index = node_key_index(&graph.schema, &compute_node_numbering(graph), current_category, &node.id);
    let body_html = render_body(node, 1, Some(&key_index));

    // Replace [edge:<id>] with relative links
    let numbering = compute_node_numbering(graph);
    let body_with_links = replace_edge_refs(graph, &body_html, current_category, &numbering, base_url);

    // Backlinks — computed from [edge:X] refs in every node body
    let backlinks = render_backlinks(graph, &node.id, current_category, &numbering, base_url);

    // TOC link — always points to the root index page
    let toc_link = root_index_link(base_url, current_category);

    let display_label = node_display_label(&graph.schema, &numbering, node);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>{display_label} — graphite</title>
<style>
{css}
</style>
</head>
<body>
<p class="brand">Graphite</p>
<p class="node-meta"><strong>{display_label}</strong> · <a href="{toc_link}">↑ index</a></p>
{body_with_links}
{backlinks}
</body>
</html>"#,
        display_label = display_label,
        toc_link = toc_link,
        body_with_links = body_with_links,
        backlinks = backlinks,
        css = css,
    )
}

// ---------------------------------------------------------------------------
// Markdown → HTML via pulldown-cmark, with heading prefix injection
// ---------------------------------------------------------------------------

fn render_body(node: &Node, heading_base: usize, heading_prefix: Option<&str>) -> String {
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
    let mut transformed = Vec::new();
    let mut in_first_heading = false;
    let mut first_heading_seen = false;
    let mut heading_prefix_injected = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                let new_level = offset_heading(level, offset);
                transformed.push(Event::Start(Tag::Heading {
                    level: new_level,
                    id,
                    classes,
                    attrs,
                }));
                if !first_heading_seen {
                    first_heading_seen = true;
                    in_first_heading = true;
                    heading_prefix_injected = false;
                }
            }
            Event::End(TagEnd::Heading(level)) => {
                let new_level = offset_heading(level, offset);
                if in_first_heading {
                    in_first_heading = false;
                }
                transformed.push(Event::End(TagEnd::Heading(new_level)));
            }
            Event::Text(text) if in_first_heading && !heading_prefix_injected => {
                if let Some(prefix) = heading_prefix {
                    transformed.push(Event::Text(format!("{} {}", prefix, text).into()));
                } else {
                    transformed.push(Event::Text(text));
                }
                heading_prefix_injected = true;
            }
            Event::Code(text) if in_first_heading && !heading_prefix_injected => {
                if let Some(prefix) = heading_prefix {
                    transformed.push(Event::Text(format!("{} ", prefix).into()));
                }
                transformed.push(Event::Code(text));
                heading_prefix_injected = true;
            }
            other => transformed.push(other),
        }
    }

    html::push_html(&mut out, transformed.into_iter());
    out
}

fn offset_heading(level: pulldown_cmark::HeadingLevel, offset: usize) -> pulldown_cmark::HeadingLevel {
    use pulldown_cmark::HeadingLevel;
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
// [edge:<id>] → <a href="RELATIVE_PATH">LABEL</a>
// ---------------------------------------------------------------------------

fn replace_edge_refs(
    graph: &Graph,
    html: &str,
    current_category: &str,
    numbering: &NodeNumbering,
    base_url: &str,
) -> String {
    let marker = "[edge:";
    let mut result = String::new();
    let mut pos = 0;

    while let Some(start) = html[pos..].find(marker) {
        result.push_str(&html[pos..pos + start]);
        let content_start = pos + start + marker.len();
        if let Some(end) = html[content_start..].find(']') {
            let id = html[content_start..content_start + end].trim();
            if let Some(target) = graph.nodes.get(id) {
                let href = relative_link(base_url, current_category, &target.category, &target.id);
                let label = node_display_label(&graph.schema, numbering, target);
                result.push_str(&format!(
                    r#"<a href="{href}">{label}</a>"#,
                    href = href,
                    label = label,
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
// Backlinks — computed from [edge:X] refs in each node body
// ---------------------------------------------------------------------------

fn render_backlinks(
    graph: &Graph,
    node_id: &str,
    current_category: &str,
    numbering: &NodeNumbering,
    base_url: &str,
) -> String {
    let mut backlink_ids: Vec<String> = Vec::new();

    for node in graph.nodes.values() {
        if node.id == node_id {
            continue;
        }
        // Scan the body for [edge:<node_id>]
        if body_refs_node(&node.body, node_id) {
            backlink_ids.push(node.id.clone());
        }
    }

    if backlink_ids.is_empty() {
        return String::new();
    }

    let items: String = backlink_ids
        .iter()
        .map(|id| {
            let target_node = graph.nodes.get(id.as_str());
            let category = target_node.map(|n| n.category.as_str()).unwrap_or("unknown");
            let href = relative_link(base_url, current_category, category, id);
            let label = if let Some(n) = target_node {
                node_display_label(&graph.schema, numbering, n)
            } else {
                id.to_string()
            };
            format!("<li><a href=\"{href}\">{label}</a></li>")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"<div class="backlinks"><h3>Referenced by</h3><ul>{items}</ul></div>"#)
}

/// Check if `body` contains `[edge:<target_id>]`.
fn body_refs_node(body: &str, target_id: &str) -> bool {
    let marker = format!("[edge:{}]", target_id);
    body.contains(&marker)
}

// ---------------------------------------------------------------------------
// Root index page
// ---------------------------------------------------------------------------

fn render_root_index(
    graph: &Graph,
    schema: &Schema,
    numbering: &NodeNumbering,
    css: &str,
    base_url: &str,
) -> String {
    let mut toc_sections = String::new();

    // Group by category and build a TOC section per category
    let mut categories: Vec<String> = group_by_category(graph).into_keys().collect();
    categories.sort();

    for category in &categories {
        let category_key = schema
            .categories
            .get(category.as_str())
            .map(|k| k.key.as_str())
            .unwrap_or("??");

        let mut nodes: Vec<&Node> = graph
            .nodes
            .values()
            .filter(|n| n.category == *category)
            .collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let mut items: Vec<String> = Vec::new();
        for n in &nodes {
            let label = node_display_label(schema, numbering, n);
            let href = relative_link(base_url, "index", category, &n.id);
            items.push(format!("<li><a href=\"{href}\">{label}</a></li>"));
        }

        toc_sections.push_str(&format!(
            r#"<h2>{} ({})</h2><ul>{}</ul>"#,
            category,
            category_key,
            items.join("\n")
        ));
    }

    let root_node = graph.nodes.get("root");
    let body_html = root_node.map(|n| {
        let raw = render_body(n, 1, None);
        replace_edge_refs(graph, &raw, "index", numbering, base_url)
    });

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Table of Contents — graphite</title>
<style>
{css}
</style>
</head>
<body>
<p class="brand">Graphite</p>
<h1>Table of Contents</h1>

{toc_sections}

{body}

</body>
</html>"#,
        toc_sections = toc_sections,
        body = body_html.unwrap_or_default(),
        css = css,
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
category: spec\n\
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
category: service\n\
---\n\
# Service\n\n\
Body with [edge:root].\n",
            )
            .expect("svc"),
        );

        g
    }

    #[test]
    fn renders_without_crashing() {
        let g = sample_graph();
        let rendered =
            render_graph(&g, None, style::DEFAULT_CSS, "").expect("render should succeed");
        let svc_page = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        assert!(
            svc_page.html.contains("<h1"),
            "node without depth offset should use h1: {}",
            svc_page.html
        );
    }

    #[test]
    fn edge_refs_replaced_with_relative_paths() {
        let g = sample_graph();
        let rendered = render_graph(&g, None, style::DEFAULT_CSS, "").expect("render");

        let svc = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        // [edge:root] should become a relative link from service/svc.html to spec/root.html
        assert!(
            svc.html.contains(r#"href="../spec/root.html""#),
            "edge ref should be a relative anchor: {}",
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
category: service\n\
---\n\
# Test\n\n\
See [edge:nonexistent].\n",
            )
            .expect("test"),
        );

        let rendered = render_graph(&g, None, style::DEFAULT_CSS, "").expect("render");
        let page = &rendered.pages[0];
        assert!(
            page.html.contains("nonexistent?"),
            "broken edge should show indicator"
        );
    }

    #[test]
    fn backlinks_computed_from_body_refs() {
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
See [edge:svc].\n",
            )
            .expect("root"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc\n\
category: service\n\
---\n\
# Service\n",
            )
            .expect("svc"),
        );

        let numbering = compute_node_numbering(&g);
        // svc is referenced by root via [edge:svc] in root's body
        let html = render_backlinks(&g, "svc", "service", &numbering, "");
        assert!(
            html.contains("Referenced by"),
            "backlinks for svc should show root's reference: {}",
            html
        );
        assert!(html.contains("root"), "backlinks should mention root");
    }

    #[test]
    fn node_label_format() {
        let schema = SchemaParser::default_schema();
        let mut numbering = HashMap::new();
        let mut service_nums = HashMap::new();
        service_nums.insert("svc".to_string(), 1usize);
        numbering.insert("service".to_string(), service_nums);

        let label = node_key_index(&schema, &numbering, "service", "svc");
        assert_eq!(label, "SVC-1", "label should be key-index");
    }

    #[test]
    fn node_title_uses_first_heading_text() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        let n = NodeParser::parse(
            "\
---\n\
id: audience-requirement\n\
category: requirement\n\
---\n\
# Audience Requirement\n\n\
Body\n",
        )
        .expect("node");
        g.add_node(n.clone());

        assert_eq!(node_title(&n), "Audience Requirement");
    }

    #[test]
    fn display_label_is_key_index_plus_title() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema.clone());
        let node = NodeParser::parse(
            "\
---\n\
id: audience-requirement\n\
category: requirement\n\
---\n\
# Audience Requirement\n\n\
Body\n",
        )
        .expect("node");
        g.add_node(node.clone());

        let numbering = compute_node_numbering(&g);
        let label = node_display_label(&schema, &numbering, &node);
        assert_eq!(label, "REQ-1 Audience Requirement");
    }

    #[test]
    fn relative_link_same_category() {
        assert_eq!(relative_link("", "service", "service", "svc-1"), "svc-1.html");
    }

    #[test]
    fn relative_link_diff_category() {
        assert_eq!(
            relative_link("", "service", "adr", "adr-1"),
            "../adr/adr-1.html"
        );
    }

    #[test]
    fn root_index_link_with_base_url() {
        assert_eq!(root_index_link("/base", "service"), "/base/index.html");
        assert_eq!(root_index_link("", "service"), "../index.html");
    }

    #[test]
    fn rendered_pages_have_base_url_prefix() {
        let g = sample_graph();
        let rendered =
            render_graph(&g, None, style::DEFAULT_CSS, "/test/").expect("render");
        // Node page with [edge:root] — cross-category link from service to spec
        let svc = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        assert!(
            svc.html.contains(r#"href="/test/spec/root.html""#),
            "cross-category edge ref should include base_url prefix: {}",
            svc.html
        );
        assert!(
            svc.html.contains(r#"href="/test/index.html""#),
            "toc link should include base_url prefix: {}",
            svc.html
        );
    }

    #[test]
    fn root_index_shows_all_categories() {
        let g = sample_graph();
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;

        let html = render_root_index(&g, schema, &numbering, style::DEFAULT_CSS, "");
        assert!(
            html.contains("Table of Contents"),
            "root index should say 'Table of Contents': {}",
            html
        );
        assert!(
            html.contains("Root body"),
            "root index should contain the root node's body"
        );
        assert!(
            html.contains("spec"),
            "root index should list the spec category"
        );
        assert!(
            html.contains("service"),
            "root index should list the service category"
        );
    }

    #[test]
    fn node_page_heading_has_key_index_and_title() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: audience-requirement\n\
category: requirement\n\
---\n\
# Audience Requirement\n\n\
Body\n",
            )
            .expect("audience-requirement"),
        );

        let rendered = render_graph(&g, None, style::DEFAULT_CSS, "").expect("render");
        let page = rendered
            .pages
            .iter()
            .find(|p| p.id == "audience-requirement")
            .expect("audience page");

        assert!(
            page.html.contains("REQ-1 Audience Requirement"),
            "node title/label should include key-index + title: {}",
            page.html
        );
    }

    #[test]
    fn body_refs_node_works() {
        assert!(body_refs_node("See [edge:svc-1] for details", "svc-1"));
        assert!(!body_refs_node("See [edge:svc-2] for details", "svc-1"));
        assert!(!body_refs_node("No refs here", "svc-1"));
    }
}
