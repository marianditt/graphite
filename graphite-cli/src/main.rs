use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use graphite_core::anchor_scanner::AnchorScanner;
use graphite_core::config::Config;
use graphite_core::node_parser::NodeParser;
use graphite_core::schema::SchemaParser;
use graphite_core::sidecar::SidecarResolver;
use graphite_core::validation::ValidationEngine;
use graphite_core::{Diagnostic, Graph, Severity};

#[derive(Parser)]
#[command(
    name = "graphite",
    about = "A compiled knowledge graph for software engineering"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Graphite project scaffold
    Init {
        path: String,
        #[arg(long)]
        force: bool,
    },
    /// Validate a graph directory
    Validate {
        path: String,
        #[arg(long)]
        focus: Option<String>,
        #[arg(long)]
        first: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show context for a node (slice relevant for a specific phase)
    Context {
        id: String,
        #[arg(long)]
        phase: Option<String>,
        #[arg(default_value = "graph")]
        graph_dir: String,
    },
    /// Compute a work order for implementing or modifying a node
    Plan {
        id: String,
        #[arg(default_value = "graph")]
        graph_dir: String,
    },
    /// Show knowledge-level changes since a git ref
    Diff {
        #[arg(long, default_value = "HEAD")]
        from: String,
        #[arg(long)]
        json: bool,
    },
    /// Render a graph directory to static HTML documentation
    Render {
        #[arg(default_value = "graph")]
        graph_dir: String,
        #[arg(long, short, default_value = "docs")]
        output: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let config = Config::load_or_default(Path::new(".")).unwrap_or_default();

    match cli.command {
        Commands::Init { path, force } => cmd_init(&path, force),
        Commands::Validate {
            path,
            focus,
            first,
            json,
        } => {
            if !cmd_validate(&path, focus.as_deref(), first, json, &config) {
                std::process::exit(1);
            }
        }
        Commands::Context {
            id,
            phase,
            graph_dir,
        } => {
            cmd_context(&id, phase.as_deref(), &graph_dir);
        }
        Commands::Plan { id, graph_dir } => cmd_plan(&id, &graph_dir),
        Commands::Diff { from, json } => cmd_diff(&from, json),
        Commands::Render { graph_dir, output } => cmd_render(&graph_dir, &output, &config),
    }
}

// ---------------------------------------------------------------------------
// Shared: load .node files from a directory into a Graph
// ---------------------------------------------------------------------------

fn load_graph(graph_dir: &str) -> Result<Graph, Vec<Diagnostic>> {
    let schema = SchemaParser::default_schema();
    let mut graph = Graph::new(schema);
    let mut errors = Vec::new();

    let dir = Path::new(graph_dir);
    if !dir.is_dir() {
        errors.push(Diagnostic {
            rule: "graph-not-found".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(graph_dir.to_string()),
            detail: format!("Graph directory '{graph_dir}' not found"),
            fix: "Run `graphite init` or point to a valid graph directory.".into(),
            example: None,
            hint: "The graph directory should contain .node files.".into(),
        });
        return Err(errors);
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(Diagnostic {
                rule: "read-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(graph_dir.to_string()),
                detail: format!("Cannot read directory '{graph_dir}': {e}"),
                fix: "Check directory permissions.".into(),
                example: None,
                hint: "Graph directory must be readable.".into(),
            });
            return Err(errors);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        collect_node_files(&path, &mut graph, &mut errors);
    }

    if errors.is_empty() {
        Ok(graph)
    } else {
        Err(errors)
    }
}

fn collect_node_files(path: &Path, graph: &mut Graph, errors: &mut Vec<Diagnostic>) {
    if path.is_dir() {
        #[allow(clippy::collapsible_if)]
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                return;
            }
        }
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                collect_node_files(&entry.path(), graph, errors);
            }
        }
        return;
    }

    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("node") && ext != Some("index") {
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            errors.push(Diagnostic {
                rule: "read-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(path.to_string_lossy().into()),
                detail: format!("Cannot read '{}': {e}", path.display()),
                fix: "Ensure the file is readable UTF-8.".into(),
                example: None,
                hint: "Graph files must be valid UTF-8.".into(),
            });
            return;
        }
    };

    match NodeParser::parse(&content) {
        Ok(node) => graph.add_node(node),
        Err(diag) => {
            let mut with_file = diag;
            with_file.file = Some(path.to_string_lossy().into());
            errors.push(with_file);
        }
    }
}

// ---------------------------------------------------------------------------
// Evidence resolution
// ---------------------------------------------------------------------------

fn resolve_evidence(scan_paths: &[String]) -> HashMap<String, Vec<(PathBuf, usize)>> {
    let mut evidence = HashMap::new();

    for scan_path in scan_paths {
        if let Ok(scanned) = AnchorScanner::scan(Path::new(scan_path)) {
            for (id, locs) in scanned {
                evidence.entry(id).or_insert_with(Vec::new).extend(locs);
            }
        }
        if let Ok(sidecars) = SidecarResolver::resolve(Path::new(scan_path)) {
            for (id, locs) in sidecars {
                evidence.entry(id).or_insert_with(Vec::new).extend(locs);
            }
        }
    }

    evidence
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

fn cmd_validate(graph_dir: &str, focus: Option<&str>, first: bool, json: bool, config: &Config) -> bool {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            output_diagnostics(&errors, focus, first, json);
            return false;
        }
    };

    let engine = ValidationEngine;
    let mut diagnostics = engine.validate(&graph);

    let evidence = resolve_evidence(&config.scan);
    diagnostics.extend(engine.check_evidence_anchors(&graph, &evidence));

    if diagnostics.is_empty() {
        println!("✓ graph is valid");
        return true;
    }

    output_diagnostics(&diagnostics, focus, first, json);
    false
}

fn output_diagnostics(diags: &[Diagnostic], focus: Option<&str>, first: bool, json: bool) {
    let mut filtered: Vec<&Diagnostic> = diags.iter().collect();

    if let Some(focus_id) = focus {
        filtered.retain(|d| d.node_id.as_deref() == Some(focus_id));
    }

    if first {
        filtered.truncate(1);
    }

    if json {
        let json_str = serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| "[]".into());
        println!("{json_str}");
        return;
    }

    for d in &filtered {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        println!("[{sev}] {}: {}", d.rule, d.detail);
        println!("  fix: {}", d.fix);
        if let Some(example) = &d.example {
            println!("  example:\n{}", example);
        }
        println!("  hint: {}", d.hint);
        if first {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

fn cmd_init(path: &str, force: bool) {
    let root = Path::new(path);
    let graph_dir = root.join("graph");

    if graph_dir.exists() && !force {
        eprintln!(
            "error: '{}/graph' already exists (use --force to overwrite)",
            root.display()
        );
        std::process::exit(1);
    }

    let dirs = [
        "graph/root",
        "graph/requirement",
        "graph/adr",
        "graph/service",
        "graph/test",
    ];
    for d in &dirs {
        let _ = fs::create_dir_all(root.join(d));
    }

    // root/root.node
    write_node(
        &graph_dir.join("root/root.node"),
        "root",
        "index",
        &[("contains", &["requirement", "adr", "service", "test"])],
        "A compiled knowledge graph for software engineering.",
        Some("general"),
    );

    // requirement/requirement.index
    write_node(
        &graph_dir.join("requirement/requirement.index"),
        "requirement",
        "index",
        &[("contains", &["compiler-requirement"])],
        "Requirement index",
        Some("requirement"),
    );

    write_node(
        &graph_dir.join("requirement/compiler-requirement.node"),
        "compiler-requirement",
        "requirement",
        &[],
        "The compiler must parse .node files, validate the graph, resolve evidence anchors, and render HTML documentation.",
        None,
    );

    // adr/adr.index
    write_node(
        &graph_dir.join("adr/adr.index"),
        "adr",
        "index",
        &[("contains", &["compiler-pipeline"])],
        "Architecture Decision Records index",
        Some("adr"),
    );

    write_node(
        &graph_dir.join("adr/compiler-pipeline.node"),
        "compiler-pipeline",
        "adr",
        &[("relates_to", &["compiler-requirement"])],
        "The compiler pipeline has four stages:\n\n1. **Parse**: Read .node files into a Graph\n2. **Validate**: Run structural checks\n3. **Resolve**: Match evidence anchors to source\n4. **Render**: Generate static HTML\n\nRelated: [edge:compiler-requirement]",
        None,
    );

    // service/service.index
    write_node(
        &graph_dir.join("service/service.index"),
        "service",
        "index",
        &[("contains", &["compiler"])],
        "Service index",
        Some("service"),
    );

    write_node(
        &graph_dir.join("service/compiler.node"),
        "compiler",
        "service",
        &[],
        "A Rust CLI that implements the graphite compiler pipeline.",
        None,
    );

    // test/test.index
    write_node(
        &graph_dir.join("test/test.index"),
        "test",
        "index",
        &[("contains", &["compiler-tests"])],
        "Test index",
        Some("test"),
    );

    write_node(
        &graph_dir.join("test/compiler-tests.node"),
        "compiler-tests",
        "test",
        &[],
        "Tests for the graphite compiler pipeline.",
        None,
    );

    println!("✓ created Graphite project at '{}'", root.display());
}

fn write_node(
    path: &Path,
    id: &str,
    kind: &str,
    edges: &[(&str, &[&str])],
    body: &str,
    of_kind: Option<&str>,
) {
    let mut frontmatter = format!("---\nid: {id}\nkind: {kind}\n");
    if let Some(ok) = of_kind {
        frontmatter.push_str(&format!("metadata:\n  of_kind: {ok}\n"));
    }
    if !edges.is_empty() {
        frontmatter.push_str("edges:\n");
        for (edge_kind, targets) in edges {
            frontmatter.push_str(&format!("  {edge_kind}:\n"));
            for t in *targets {
                frontmatter.push_str(&format!("    - {t}\n"));
            }
        }
    }
    frontmatter.push_str("---\n");
    let content = format!("{frontmatter}{body}\n");
    let _ = fs::write(path, content);
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct DiffOutput {
    new: Vec<String>,
    changed: Vec<String>,
    removed: Vec<String>,
}

fn cmd_diff(from: &str, json: bool) {
    let repo_root = match git_root() {
        Some(r) => r,
        None => {
            eprintln!("error: not in a git repository");
            std::process::exit(1);
        }
    };

    let output = std::process::Command::new("git")
        .args(["diff", "--name-status", from, "--", "*.node"])
        .current_dir(&repo_root)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            eprintln!("error: git diff failed (is '{from}' a valid ref?)");
            std::process::exit(1);
        }
    };

    let mut new_nodes = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let status = parts[0];
        let file = parts[1];
        let file = file.trim_start_matches("graph/");
        match status.chars().next() {
            Some('A') => new_nodes.push(file.to_string()),
            Some('M' | 'R') => changed.push(file.to_string()),
            Some('D') => removed.push(file.to_string()),
            _ => {}
        }
    }

    let diff = DiffOutput {
        new: new_nodes,
        changed,
        removed,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&diff).unwrap());
    } else {
        for f in &diff.new {
            println!("  NEW    {f}");
        }
        for f in &diff.changed {
            println!("  CHANGED {f}");
        }
        for f in &diff.removed {
            println!("  REMOVED {f}");
        }
        if diff.new.is_empty() && diff.changed.is_empty() && diff.removed.is_empty() {
            println!("  (no changes)");
        }
    }
}

fn git_root() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(PathBuf::from(path))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// context
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct ContextOutput {
    read: Vec<ContextNode>,
    modify: Vec<ContextNode>,
    validate: Vec<ContextNode>,
}

#[derive(serde::Serialize)]
struct ContextNode {
    id: String,
    kind: String,
    file: String,
    body: String,
}

fn cmd_context(id: &str, phase: Option<&str>, graph_dir: &str) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let target = match graph.nodes.get(id) {
        Some(n) => n,
        None => {
            eprintln!("error: node '{id}' not found in graph");
            std::process::exit(1);
        }
    };

    // Collect dependency nodes (incoming edges = this node depends on them)
    let mut deps: Vec<&graphite_core::Node> = Vec::new();
    for targets in target.edges.values() {
        for t in targets {
            if let Some(n) = graph.nodes.get(t.as_str())
                && !deps.iter().any(|d| d.id == n.id)
            {
                deps.push(n);
            }
        }
    }

    // Collect dependent nodes (outgoing edges = nodes that depend on this node)
    let mut dependents: Vec<&graphite_core::Node> = Vec::new();
    for node in graph.nodes.values() {
        for targets in node.edges.values() {
            if targets.iter().any(|t| t == id)
                && !dependents.iter().any(|d| d.id == node.id)
            {
                dependents.push(node);
            }
        }
    }

    // Collect evidence-related nodes
    let mut evidence_nodes: Vec<&graphite_core::Node> = Vec::new();
    for node in graph.nodes.values() {
        for (edge_kind, targets) in &node.edges {
            if edge_kind == "verified_by" && targets.iter().any(|t| t == id) {
                evidence_nodes.push(node);
            }
        }
    }

    // Phase-specific slicing
    let ctx_node = |n: &graphite_core::Node| -> ContextNode {
        ContextNode {
            id: n.id.clone(),
            kind: n.kind.clone(),
            file: format!("graph/{}/{}.node", n.kind, n.id),
            body: n.body.clone(),
        }
    };

    match phase {
        Some("understand") => {
            let output: Vec<ContextNode> = std::iter::once(target)
                .chain(deps.iter().copied())
                .map(ctx_node)
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": output })).unwrap()
            );
        }
        Some("plan") => {
            let mut all: Vec<ContextNode> = Vec::new();
            all.push(ctx_node(target));
            all.extend(deps.iter().copied().map(ctx_node));
            all.extend(dependents.iter().copied().map(ctx_node));
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": all })).unwrap()
            );
        }
        Some("implement") => {
            let mut modify: Vec<ContextNode> = Vec::new();
            modify.push(ctx_node(target));
            modify.extend(deps.iter().copied().map(ctx_node));
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "modify": modify })).unwrap()
            );
        }
        Some("validate") => {
            let output: Vec<ContextNode> = evidence_nodes
                .iter()
                .copied()
                .chain(dependents.iter().copied())
                .map(ctx_node)
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": output })).unwrap()
            );
        }
        Some(other) => {
            eprintln!(
                "error: unknown phase '{other}'. Valid phases: understand, plan, implement, validate"
            );
            std::process::exit(1);
        }
        None => {
            let output = ContextOutput {
                read: deps.iter().copied().map(ctx_node).collect(),
                modify: vec![ctx_node(target)],
                validate: evidence_nodes
                    .iter()
                    .copied()
                    .chain(dependents.iter().copied())
                    .map(ctx_node)
                    .collect(),
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
    }
}

// ---------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct PlanOutput {
    target: PlanTarget,
    work_order: Vec<WorkStep>,
}

#[derive(serde::Serialize)]
struct PlanTarget {
    id: String,
    kind: String,
}

#[derive(serde::Serialize)]
struct WorkStep {
    order: usize,
    action: String,
    node: String,
    source_file: String,
    why: String,
}

fn cmd_plan(id: &str, graph_dir: &str) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let target = match graph.nodes.get(id) {
        Some(n) => n,
        None => {
            eprintln!("error: node '{id}' not found in graph");
            std::process::exit(1);
        }
    };

    let mut work_order: Vec<WorkStep> = Vec::new();
    let mut order = 1;

    // Phase 1: Read context nodes (dependencies)
    let mut seen = std::collections::HashSet::new();
    for targets in target.edges.values() {
        for t in targets {
            if seen.insert(t.clone()) {
                let source_file = format!("graph/{kind}/{t}.node", kind = target.kind);
                work_order.push(WorkStep {
                    order,
                    action: "read".into(),
                    node: t.clone(),
                    source_file,
                    why: format!("Understand context for '{}'", t),
                });
                order += 1;
            }
        }
    }

    // Phase 2: Modify the target node
    let target_file = format!("graph/{kind}/{id}.node", kind = target.kind);
    work_order.push(WorkStep {
        order,
        action: "modify".into(),
        node: id.to_string(),
        source_file: target_file,
        why: format!("Implement or modify '{}'", id),
    });
    order += 1;

    // Phase 3: Evidence anchors
    work_order.push(WorkStep {
        order,
        action: "anchor".into(),
        node: id.to_string(),
        source_file: String::new(),
        why: format!(
            "Add @graphite:evidence anchors for '{}' in source files",
            id
        ),
    });
    order += 1;

    // Phase 4: Validate
    work_order.push(WorkStep {
        order,
        action: "validate".into(),
        node: id.to_string(),
        source_file: String::new(),
        why: "Run `graphite validate --first` and fix errors iteratively".into(),
    });

    let output = PlanOutput {
        target: PlanTarget {
            id: id.to_string(),
            kind: target.kind.clone(),
        },
        work_order,
    };

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

fn cmd_render(graph_dir: &str, output: &str, config: &Config) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let evidence = resolve_evidence(&config.scan);
    let output_path = Path::new(output);

    if let Err(d) = graphite_render::render_to_dir(&graph, &evidence, output_path) {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        eprintln!("[{sev}] {}: {}", d.rule, d.detail);
        eprintln!("  fix: {}", d.fix);
        eprintln!("  hint: {}", d.hint);
        std::process::exit(1);
    }

    println!(
        "✓ rendered {} nodes to '{}'",
        graph.nodes.len(),
        output
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn self_hosting_validation() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let graph_dir = workspace_root.join("graph");
        let config = Config::default();
        let ok = cmd_validate(graph_dir.to_str().unwrap(), None, false, false, &config);
        assert!(ok, "self-hosting validation failed on graph/");
    }

    #[test]
    fn full_pipeline_integration() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let graph_dir = root.join("graph");

        cmd_init(root.to_str().unwrap(), false);
        assert!(graph_dir.exists(), "init should create graph/ directory");

        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("compiler.rs"),
            "// @graphite:evidence compiler-impl\nfn compile() {}\n",
        )
        .unwrap();

        let req_path = graph_dir.join("requirement/compiler-requirement.node");
        fs::write(
            &req_path,
            r#"---
id: compiler-requirement
kind: requirement
edges:
  implemented_by:
    - compiler
  verified_by:
    - compiler-tests
---
The compiler must parse and validate [edge:compiler] and [edge:compiler-tests].
"#,
        )
        .unwrap();

        let config = Config::default();
        let valid = cmd_validate(graph_dir.to_str().unwrap(), None, false, false, &config);
        assert!(valid, "integration: validate should pass with valid graph");

        // Call context --phase implement and plan on the requirement node.
        // These print JSON to stdout; on error they call process::exit(1) which
        // would abort the test, so completing means success.
        cmd_context(
            "compiler-requirement",
            Some("implement"),
            graph_dir.to_str().unwrap(),
        );
        cmd_plan("compiler-requirement", graph_dir.to_str().unwrap());

        let graph = load_graph(graph_dir.to_str().unwrap()).expect("integration: load graph");
        let evidence = resolve_evidence(&config.scan);
        let output_dir = root.join("html");
        graphite_render::render_to_dir(&graph, &evidence, &output_dir)
            .expect("integration: render should succeed");

        assert!(
            output_dir.join("index.html").exists(),
            "integration: root index.html should exist"
        );
        assert!(
            output_dir.join("requirement/index.html").exists(),
            "integration: requirement kind index should exist"
        );
        let compiler_page = output_dir.join("service/compiler.html");
        assert!(
            compiler_page.exists(),
            "integration: service/compiler.html should exist"
        );
        let html = fs::read_to_string(&compiler_page).unwrap();
        assert!(
            html.contains("href="),
            "integration: rendered page should have hyperlinks: {}",
            html
        );
    }
}
