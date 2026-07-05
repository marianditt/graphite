use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphite_core::{Diagnostic, Graph, Node, Schema, Severity};
use pulldown_cmark::HeadingLevel;

pub mod style;

/// Output of rendering a single node page.
pub struct RenderedNode {
    pub id: String,
    pub category: String,
    pub html: String,
}

/// Maps each node ID to its containment depth (0 = root).
pub type DepthMap = HashMap<String, usize>;

/// The rendered output for the entire graph.
pub struct RenderedGraph {
    pub pages: Vec<RenderedNode>,
    pub depth_map: DepthMap,
}

/// Per-category sequential numbering: category → node_id → 1-based index.
type NodeNumbering = HashMap<String, HashMap<String, usize>>;

// @graphite:evidence spec-render
/// Errors produced during rendering.
#[allow(clippy::result_large_err)]
pub fn render_to_dir(
    graph: &Graph,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
    output_dir: &Path,
    repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> Result<(), Diagnostic> {
    let rendered = render_graph(graph, evidence, repo_url, css, base_url)?;
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

    let numbering = compute_node_numbering(graph);
    let schema = &graph.schema;

    // Generate category index pages
    for (category, nodes) in group_by_category(graph) {
        // Skip "index" category — its ontology is different
        if category == "index" {
            continue;
        }
        let category_dir = output_dir.join(&category);
        fs::create_dir_all(&category_dir).ok();
        let index_html = render_kind_index(
            &category,
            &nodes,
            &rendered.depth_map,
            graph,
            schema,
            &numbering,
            repo_url,
            css,
            base_url,
        );
        fs::write(category_dir.join("index.html"), index_html).ok();
    }

    // Generate root index
    let root_html = render_root_index(
        graph,
        &rendered.depth_map,
        schema,
        &numbering,
        repo_url,
        css,
        base_url,
    );
    fs::write(output_dir.join("index.html"), root_html).ok();

    Ok(())
}

/// Render the entire graph to HTML pages.
#[allow(clippy::result_large_err)]
fn render_graph(
    graph: &Graph,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
    repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> Result<RenderedGraph, Diagnostic> {
    let depth_map = compute_depths(graph);
    let numbering = compute_node_numbering(graph);

    let mut pages = Vec::new();
    for node in graph.nodes.values() {
        let html = render_node_page(graph, node, &depth_map, evidence, &numbering, repo_url, css, base_url);
        pages.push(RenderedNode {
            id: node.id.clone(),
            category: node.category.clone(),
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

/// Group nodes by their category, excluding "index" and "evidence" nodes.
fn group_by_category(graph: &Graph) -> HashMap<String, Vec<&Node>> {
    let mut map: HashMap<String, Vec<&Node>> = HashMap::new();
    for node in graph.nodes.values() {
        if node.category == "index" || node.category == "evidence" {
            continue;
        }
        map.entry(node.category.clone()).or_default().push(node);
    }
    map
}

/// Compute sequential numbering per category (sorted by node ID).
fn compute_node_numbering(graph: &Graph) -> NodeNumbering {
    let mut numbering = HashMap::new();
    for (category, nodes) in group_by_category(graph) {
        let mut sorted: Vec<&Node> = nodes;
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        let mut cat_map = HashMap::new();
        for (i, node) in sorted.iter().enumerate() {
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

/// Build a link from a page of `from_category` to a category index page.
///
/// When `base_url` is non-empty, generates an absolute path (for GitHub
/// Pages subpath serving). When empty, generates a relative path (default).
///
/// Special case: `to_category == "index"` refers to the root index page, which
/// lives at the output root (`index.html`), not in `index/index.html`.
fn relative_index_link(base_url: &str, from_category: &str, to_category: &str) -> String {
    if base_url.is_empty() {
        if to_category == "index" {
            "../index.html".into()
        } else if from_category == to_category {
            "index.html".into()
        } else {
            format!("../{}/index.html", to_category)
        }
    } else {
        let base = base_url.trim_end_matches('/');
        if to_category == "index" {
            format!("{}/index.html", base)
        } else {
            format!("{}/{}/index.html", base, to_category)
        }
    }
}

// ---------------------------------------------------------------------------
// Single node page rendering
// ---------------------------------------------------------------------------

fn render_node_page(
    graph: &Graph,
    node: &Node,
    depths: &DepthMap,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
    numbering: &NodeNumbering,
    repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> String {
    let current_category = &node.category;
    let depth = depths.get(&node.id).copied().unwrap_or(0);
    let heading_base = (1 + depth).min(6); // clamp at h6

    // Build heading-depth-adjusted body
    let key_index = node_key_index(&graph.schema, numbering, current_category, &node.id);
    let body_html = render_body(node, heading_base, Some(&key_index));

    // Replace [edge:<id>] with relative links
    let body_with_links = replace_edge_refs(graph, &body_html, current_category, numbering, base_url);

    // Backlinks
    let backlinks = render_backlinks(graph, &node.id, current_category, numbering, base_url);

    // Evidence section
    let ev_section = render_evidence_section(node, evidence, repo_url);

    // TOC link — relative from {category}/{id}.html to the category index page.
    // For index-category nodes we link to their of_category index page (if known)
    // or the root page (fallback).
    let toc_link = if current_category == "index" {
        if let Some(of_category) = node.metadata.get("of_category").filter(|k| *k != "general") {
            relative_index_link(base_url, current_category, of_category)
        } else {
            relative_index_link(base_url, current_category, "index")
        }
    } else {
        relative_index_link(base_url, current_category, current_category)
    };

    let display_label = node_display_label(&graph.schema, numbering, node);

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
<p class="node-meta"><strong>{display_label}</strong> · <a href="{toc_link}">↑ {category}</a></p>
{body_with_links}
{ev_section}
{backlinks}
</body>
</html>"#,
        category = node.category,
        display_label = display_label,
        toc_link = toc_link,
        body_with_links = body_with_links,
        ev_section = ev_section,
        backlinks = backlinks,
        css = css,
    )
}

// ---------------------------------------------------------------------------
// Markdown → HTML via pulldown-cmark, with heading offset
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
// Backlinks
// ---------------------------------------------------------------------------

fn render_backlinks(
    graph: &Graph,
    node_id: &str,
    current_category: &str,
    numbering: &NodeNumbering,
    base_url: &str,
) -> String {
    let mut backlinks: Vec<&str> = Vec::new();

    for node in graph.nodes.values() {
        // Skip index-category nodes — they are parent containers whose pages are
        // the category index pages (not individual pages worth linking to).
        if node.category == "index" {
            continue;
        }
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
            let target_node = graph.nodes.get(*id);
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

    format!(r#"<div class="backlinks"><h3>Referenced by</h3><ul>{items}</ul></div>"#,)
}

// ---------------------------------------------------------------------------
// Evidence section
// ---------------------------------------------------------------------------

fn render_evidence_section(
    node: &Node,
    evidence: &HashMap<String, Vec<(PathBuf, usize)>>,
    repo_url: Option<&str>,
) -> String {
    let mut items = String::new();
    let mut seen_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut seen_locations: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();

    for (edge_kind, targets) in &node.edges {
        if edge_kind != "evidence" {
            continue;
        }
        for ev_id in targets {
            if !seen_ids.insert(ev_id.as_str()) {
                continue;
            }
            if let Some(locations) = evidence.get(ev_id) {
                for (path, line) in locations {
                    if !seen_locations.insert((path.to_string_lossy().to_string(), *line)) {
                        continue;
                    }
                    let display_path = path.display();
                    if let Some(base) = repo_url {
                        items.push_str(&format!(
                            r#"<li><a href="{base}/blob/main/{path}#L{line}"><code>{display_path}</code> line {line}</a></li>"#,
                            base = base,
                            path = path.display(),
                            line = line,
                            display_path = display_path,
                        ));
                    } else {
                        items.push_str(&format!(
                            r#"<li><code>{display_path}</code> line {line}</li>"#,
                            display_path = display_path,
                            line = line,
                        ));
                    }
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
        // Category index page
// ---------------------------------------------------------------------------

fn render_kind_index(
    category: &str,
    nodes: &[&Node],
    depths: &DepthMap,
    graph: &Graph,
    schema: &Schema,
    numbering: &NodeNumbering,
    _repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> String {
    let index_body: String = graph
        .nodes
        .values()
        .find(|n| n.category == "index" && n.metadata.get("of_category").map(|s| s.as_str()) == Some(category))
        .map(|idx_node| {
            let raw = render_body(idx_node, 0, None);
            replace_edge_refs(graph, &raw, category, numbering, base_url)
        })
        .unwrap_or_default();

    // Build TOC items with sequential numbering
    let mut items: Vec<String> = nodes
        .iter()
        .map(|n| {
            let depth = depths.get(&n.id).copied().unwrap_or(0);
            let indent = "  ".repeat(depth);
            let label = node_display_label(schema, numbering, n);
            if base_url.is_empty() {
                format!(
                    r#"{}<li><a href="{}.html">{}</a></li>"#,
                    indent, n.id, label
                )
            } else {
                let base = base_url.trim_end_matches('/');
                format!(
                    r#"{}<li><a href="{}/{}/{}.html">{}</a></li>"#,
                    indent, base, category, n.id, label
                )
            }
        })
        .collect();
    items.sort();

    let toc_items = items.join("\n");
    let root_link = relative_index_link(base_url, category, "index"); // from category/ to root = ../index.html
    let category_key = schema
        .categories
        .get(category)
        .map(|k| k.key.as_str())
        .unwrap_or("??");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Table of Contents: {category} — graphite</title>
<style>
{css}
</style>
</head>
<body>
<p class="brand">Graphite</p>
<h1>Table of Contents: {category} ({category_key})</h1>

<ul>
{toc_items}
</ul>

{index_body}

<a href="{root_link}">← Graph root</a>
</body>
</html>"#,
        category = category,
        category_key = category_key,
        toc_items = toc_items,
        index_body = index_body,
        root_link = root_link,
        css = css,
    )
}

// ---------------------------------------------------------------------------
// Root index page
// ---------------------------------------------------------------------------

fn render_root_index(
    graph: &Graph,
    _depths: &DepthMap,
    schema: &Schema,
    numbering: &NodeNumbering,
    _repo_url: Option<&str>,
    css: &str,
    base_url: &str,
) -> String {
    // Get all non-index categories (for detecting categories not in the root's contains list).
    let nodes_by_category = group_by_category(graph);

    // Get the root node by ID ("root") and read its `contains` edges to determine
    // the display order of categories.
    let root_node = graph.nodes.get("root");

    // Build TOC from the root node's directly contained children, preserving
    // the order defined by the `contains` edges.
    let mut toc_items: Vec<String> = Vec::new();
    let mut seen_categories: Vec<String> = Vec::new();
    if let Some(root) = root_node {
        if let Some(children) = root.edges.get("contains") {
            let mut per_category_index: HashMap<String, usize> = HashMap::new();
            for child_id in children {
                if let Some(child) = graph.nodes.get(child_id.as_str()) {
                    // Child is an index node with of_category metadata — use that to
                    // find the category key and link to its category index page.
                    let of_category = child
                        .metadata
                        .get("of_category")
                        .map(|s| s.as_str())
                        .unwrap_or(child_id);
                    let key = schema
                        .categories
                        .get(of_category)
                        .map(|k| k.key.as_str())
                        .unwrap_or("??");
                    let index = {
                        let entry = per_category_index.entry(of_category.to_string()).or_insert(0);
                        *entry += 1;
                        *entry
                    };
                    let title = node_title(child);
                    let label = format!("{}-{} {}", key, index, title);
                    seen_categories.push(of_category.to_string());
                    if base_url.is_empty() {
                        toc_items.push(format!(
                            r#"<li><a href="{category}/index.html">{label}</a></li>"#,
                            category = of_category,
                            label = label,
                        ));
                    } else {
                        let base = base_url.trim_end_matches('/');
                        toc_items.push(format!(
                            r#"<li><a href="{base}/{category}/index.html">{label}</a></li>"#,
                            base = base,
                            category = of_category,
                            label = label,
                        ));
                    }
                }
            }
        }

        // Append categories that exist in the graph but are NOT listed in the
        // root node's contains edges (sorted alphabetically at the end).
        let mut remaining_categories: Vec<&String> = nodes_by_category
            .keys()
            .filter(|cat| !seen_categories.contains(cat))
            .collect();
        remaining_categories.sort();
        for category in remaining_categories {
            let key = schema
                .categories
                .get(category.as_str())
                .map(|k| k.key.as_str())
                .unwrap_or("??");
            let title = graph
                .nodes
                .values()
                .find(|n| {
                    n.category == "index"
                        && n.metadata.get("of_category").map(|s| s.as_str()) == Some(category.as_str())
                })
                .map(|n| node_title(n))
                .unwrap_or_else(|| category.clone());
            let label = format!("{}-1 {}", key, title);
            if base_url.is_empty() {
                toc_items.push(format!(
                    r#"<li><a href="{category}/index.html">{label}</a></li>"#,
                    category = category,
                    label = label,
                ));
            } else {
                let base = base_url.trim_end_matches('/');
                toc_items.push(format!(
                    r#"<li><a href="{base}/{category}/index.html">{label}</a></li>"#,
                    base = base,
                    category = category,
                    label = label,
                ));
            }
        }

        // Root index body — rendered with edge ref resolution.
        let body_html = render_body(root, 0, None);
        let body_with_links = replace_edge_refs(graph, &body_html, "index", numbering, base_url);

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

<ul>
{toc}
</ul>

<div class="index-body">
{body}
</div>

</body>
</html>"#,
            toc = toc_items.join("\n"),
            body = body_with_links,
            css = css,
        )
    } else {
        // Fallback: no root node found
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>graphite — knowledge graph</title>
<style>
{css}
</style>
</head>
<body>
<p class="brand">Graphite</p>
<h1>graphite</h1>
<p>A compiled knowledge graph for software engineering.</p>
</body>
</html>"#,
            css = css,
        )
    }
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
category: index\n\
edges:\n  contains:\n    - svc-index\n\
metadata:\n  of_category: general\n\
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
id: svc-index\n\
category: index\n\
edges:\n  contains:\n    - svc\n\
metadata:\n  of_category: service\n\
---\n\
# Service Index\n\n\
Service overview.\n",
            )
            .expect("svc-index"),
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
    fn heading_depth_root_is_h1() {
        let g = sample_graph();
        let depths = compute_depths(&g);
        assert_eq!(depths.get("root"), Some(&0));
        assert_eq!(depths.get("svc-index"), Some(&1));
        assert_eq!(depths.get("svc"), Some(&2));
    }

    #[test]
    fn renders_without_crashing() {
        let g = sample_graph();
        let evidence = HashMap::new();
        let rendered =
            render_graph(&g, &evidence,             None, style::DEFAULT_CSS, "").expect("render should succeed");
        // svc is at depth 2 → heading_base = 3 → offset headings to h3
        let svc_page = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        assert!(
            svc_page.html.contains("<h3"),
            "svc (depth 2) heading should be offset to h3: {}",
            svc_page.html
        );
        // svc-index (category: index) is also rendered as a page
        let idx_page = rendered.pages.iter().find(|p| p.id == "svc-index").unwrap();
        assert!(
            idx_page.html.contains("<h2"),
            "svc-index (depth 1) heading should be offset to h2"
        );
    }

    #[test]
    fn edge_refs_replaced_with_relative_paths() {
        let g = sample_graph();
        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");

        let svc = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        // [edge:root] should become a relative link from service/svc.html to index/root.html
        assert!(
            svc.html.contains(r#"href="../index/root.html""#),
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

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");
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
category: service\n\
edges:\n  evidence:\n    - ev-auth\n---\n\
# Service\n",
            )
            .expect("svc"),
        );

        let mut evidence = HashMap::new();
        evidence.insert("ev-auth".into(), vec![(PathBuf::from("src/main.rs"), 42)]);

        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");
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
    fn evidence_with_repo_url() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc\n\
category: service\n\
edges:\n  evidence:\n    - ev-auth\n---\n\
# Service\n",
            )
            .expect("svc"),
        );

        let mut evidence = HashMap::new();
        evidence.insert("ev-auth".into(), vec![(PathBuf::from("src/main.rs"), 42)]);

        let rendered = render_graph(
            &g,
            &evidence,
            Some("https://github.com/owner/repo"),
            style::DEFAULT_CSS,
            "",
        )
        .expect("render");
        let page = &rendered.pages[0];
        assert!(
            page.html
                .contains("https://github.com/owner/repo/blob/main/src/main.rs#L42"),
            "evidence link should include repo URL"
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
category: index\n\
edges:\n  contains:\n    - r1\n\
metadata:\n  of_category: general\n\
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
category: index\n\
edges:\n  contains:\n    - r{next}\n\
metadata:\n  of_category: general\n\
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
category: service\n\
---\n# Last\n"
            ))
            .expect("last"),
        );

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");

        // Deepest node should have heading clamped at h6
        let deepest = rendered.pages.iter().find(|p| p.id == "r11").unwrap();
        assert!(
            deepest.html.contains("<h6"),
            "deepest node should be h6, not lower"
        );
        // Should NOT have h7
        assert!(!deepest.html.contains("<h7"), "no h7 allowed");
    }

    #[test]
    fn kind_index_page_has_toc_title() {
        let g = sample_graph();
        let depths = compute_depths(&g);
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;
        let nodes_by_category = group_by_category(&g);
        let service_nodes = nodes_by_category.get("service").expect("service nodes");

        let html = render_kind_index(
            "service",
            service_nodes,
            &depths,
            &g,
            schema,
            &numbering,
            None,
            style::DEFAULT_CSS,
            "",
        );
        assert!(
            html.contains("Table of Contents"),
            "category index should say 'Table of Contents': {}",
            html
        );
        assert!(
            html.contains("SVC"),
            "kind index should show category key 'SVC'"
        );
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
    fn kind_index_has_body_between_title_and_toc() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        // Index node with body and of_category=service
        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc-index\n\
category: index\n\
edges:\n  contains:\n    - svc-1\n\
metadata:\n  of_category: service\n\
---\n\
# Service Overview\n\n\
This index page covers all services.\n",
            )
            .expect("svc-index"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc-1\n\
category: service\n\
---\n\
# Service One\n",
            )
            .expect("svc-1"),
        );

        let depths = compute_depths(&g);
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;
        let nodes_by_category = group_by_category(&g);
        let service_nodes = nodes_by_category.get("service").expect("service nodes");

        let html = render_kind_index(
            "service",
            service_nodes,
            &depths,
            &g,
            schema,
            &numbering,
            None,
            style::DEFAULT_CSS,
            "",
        );
        // The index node body should appear after the TOC (the <ul>)
        let ul_pos = html.find("<ul").unwrap();
        let body_pos = html.find("Service Overview").unwrap();
        assert!(body_pos > ul_pos, "body should appear after TOC list");
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
    fn relative_index_same_category() {
        assert_eq!(relative_index_link("", "service", "service"), "index.html");
    }

    #[test]
    fn relative_index_diff_category() {
        assert_eq!(relative_index_link("", "service", "adr"), "../adr/index.html");
    }

    #[test]
    fn relative_index_to_root() {
        // to_category == "index" means the root index page at output root
        assert_eq!(relative_index_link("", "service", "index"), "../index.html");
        assert_eq!(relative_index_link("", "adr", "index"), "../index.html");
    }

    #[test]
    fn relative_link_with_base_url() {
        assert_eq!(
            relative_link("/base", "service", "service", "svc-1"),
            "/base/service/svc-1.html"
        );
        assert_eq!(
            relative_link("/base/", "service", "adr", "adr-1"),
            "/base/adr/adr-1.html"
        );
    }

    #[test]
    fn relative_index_link_with_base_url() {
        assert_eq!(
            relative_index_link("/base", "service", "service"),
            "/base/service/index.html"
        );
        assert_eq!(
            relative_index_link("/base/", "service", "adr"),
            "/base/adr/index.html"
        );
        assert_eq!(
            relative_index_link("/base", "service", "index"),
            "/base/index.html"
        );
    }

    #[test]
    fn rendered_pages_have_base_url_prefix() {
        let g = sample_graph();
        let evidence = HashMap::new();
        let rendered =
            render_graph(&g, &evidence, None, style::DEFAULT_CSS, "/test/").expect("render");
        // Node page with [edge:root] — cross-category link from service to index
        let svc = rendered.pages.iter().find(|p| p.id == "svc").unwrap();
        assert!(
            svc.html.contains(r#"href="/test/index/root.html""#),
            "cross-category edge ref should include base_url prefix: {}",
            svc.html
        );
        assert!(
            svc.html.contains(r#"href="/test/service/index.html""#),
            "toc link should include base_url prefix"
        );
// Category index page
        let svc_idx = rendered.pages.iter().find(|p| p.id == "svc-index").unwrap();
        assert!(
            svc_idx.html.contains(r#"href="/test/service/index.html""#),
            "index node toc link should use base_url"
        );
    }

    #[test]
    fn kind_index_toc_links_include_category_in_base_url() {
        let g = sample_graph();
        let depths = compute_depths(&g);
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;
        let nodes_by_category = group_by_category(&g);
        let service_nodes = nodes_by_category.get("service").expect("service nodes");
        let html = render_kind_index(
            "service",
            service_nodes,
            &depths,
            &g,
            schema,
            &numbering,
            None,
            style::DEFAULT_CSS,
            "/base/",
        );
        assert!(
            html.contains(r#"href="/base/service/svc.html""#),
            "kind index TOC link should include category dir in base_url path: {}",
            html
        );
        assert!(
            html.contains(r#"href="/base/index.html""#),
            "root link should use base_url"
        );
    }

    #[test]
    fn root_index_follows_index_pattern() {
        let g = sample_graph();
        let depths = compute_depths(&g);
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;

        let html = render_root_index(&g, &depths, schema, &numbering, None, style::DEFAULT_CSS, "");
        // Should say "Table of Contents"
        assert!(
            html.contains("Table of Contents"),
            "root index should say 'Table of Contents': {}",
            html
        );
        // Should contain the root body
        assert!(
            html.contains("Root body"),
            "root index should contain the root node's body"
        );
        // Should NOT contain the old "Kinds" section
        assert!(
            !html.contains("<h2>Kinds</h2>"),
            "root index should NOT have the old Kinds section"
        );
        // Should NOT contain the old "All Nodes" section
        assert!(
            !html.contains("<h2>All Nodes</h2>"),
            "root index should NOT have the old All Nodes section"
        );
        // Should link to the service category index
        assert!(
            html.contains(r#"href="service/index.html""#),
            "root index should link to service/index.html"
        );
        assert!(
            html.contains("SVC-1 Service Index"),
            "root index toc entries should use KEY-INDEX Title format: {}",
            html
        );
    }

    #[test]
    fn toc_links_use_key_index_and_title() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: req-index\n\
category: index\n\
metadata:\n  of_category: requirement\n\
---\n\
# Requirement Index\n",
            )
            .expect("req-index"),
        );

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

        let depths = compute_depths(&g);
        let numbering = compute_node_numbering(&g);
        let schema = &g.schema;
        let nodes_by_category = group_by_category(&g);
        let req_nodes = nodes_by_category.get("requirement").expect("requirement nodes");
        let html = render_kind_index(
            "requirement",
            req_nodes,
            &depths,
            &g,
            schema,
            &numbering,
            None,
            style::DEFAULT_CSS,
            "",
        );

        assert!(
            html.contains("REQ-1 Audience Requirement"),
            "toc label should include key-index and title: {}",
            html
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

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");
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
    fn backlinks_skip_index_category_nodes() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: root\n\
category: index\n\
edges:\n  contains:\n    - svc\n\
metadata:\n  of_category: general\n\
---\n# Root\n",
            )
            .expect("root"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc\n\
category: service\n\
---\n# Service\n",
            )
            .expect("svc"),
        );

        let numbering = compute_node_numbering(&g);
        // svc is contained by root (category: index) — root should NOT appear
        // as a backlink because index-category nodes are skipped.
        let html = render_backlinks(&g, "svc", "service", &numbering, "");
        assert!(
            !html.contains("Referenced by"),
            "backlinks for svc should be empty (only root contains it): {}",
            html
        );
    }

    #[test]
    fn index_node_toc_link_uses_of_category() {
        let schema = SchemaParser::default_schema();
        let mut g = Graph::new(schema);

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: root\n\
category: index\n\
edges:\n  contains:\n    - svc-index\n\
metadata:\n  of_category: general\n\
---\n# Root\n",
            )
            .expect("root"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc-index\n\
category: index\n\
edges:\n  contains:\n    - svc-1\n\
metadata:\n  of_category: service\n\
---\n# Service Index\n",
            )
            .expect("svc-index"),
        );

        g.add_node(
            NodeParser::parse(
                "\
---\n\
id: svc-1\n\
category: service\n\
---\n# Service One\n",
            )
            .expect("svc-1"),
        );

        let evidence = HashMap::new();
        let rendered = render_graph(&g, &evidence, None, style::DEFAULT_CSS, "").expect("render");

        // The svc-index node (category: index, of_category: service) should have
        // a TOC link pointing to the service category index page.
        let idx_page = rendered
            .pages
            .iter()
            .find(|p| p.id == "svc-index")
            .expect("svc-index page");
        assert!(
            idx_page.html.contains(r#"href="../service/index.html""#),
            "index node should link to its of_category index page: {}",
            idx_page.html
        );

        // The root node (category: index, of_category: general) should link to root.
        let root_page = rendered
            .pages
            .iter()
            .find(|p| p.id == "root")
            .expect("root page");
        assert!(
            root_page.html.contains(r#"href="../index.html""#),
            "root node should link to ../index.html: {}",
            root_page.html
        );
    }
}
